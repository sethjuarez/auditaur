param(
    [int]$TimeoutSeconds = 180,
    [string]$DataDir = ""
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$cli = Join-Path $repoRoot "target\debug\auditaur.exe"
$dogfood = Join-Path $repoRoot "examples\dogfood"

$cleanupDataDir = $false
if (-not $DataDir) {
    $DataDir = Join-Path ([System.IO.Path]::GetTempPath()) ("auditaur-dogfood-smoke-" + [System.Guid]::NewGuid().ToString("N"))
    $cleanupDataDir = $true
}

function Invoke-JsonLineCommand {
    param([string[]]$CommandArgs)

    $output = & $cli @CommandArgs
    if ($LASTEXITCODE -ne 0) {
        throw "auditaur $($CommandArgs -join ' ') failed with exit code $LASTEXITCODE"
    }
    $text = ($output -join "`n").Trim()
    if (-not $text) {
        throw "Expected JSON output from auditaur $($CommandArgs -join ' ')"
    }
    try {
        return $text | ConvertFrom-Json
    }
    catch {
        $jsonLines = @($output | Where-Object { $_ -and $_.Trim().StartsWith("{") -and $_.Trim().EndsWith("}") })
        if (-not $jsonLines) {
            throw
        }
        return $jsonLines[-1] | ConvertFrom-Json
    }
}

function Invoke-DriveJson {
    param([string[]]$CommandArgs)

    return Invoke-JsonLineCommand -CommandArgs (@("drive", "--app", "auditaur-dogfood", "--active", "--json") + $CommandArgs)
}

function Assert-Condition {
    param(
        [bool]$Condition,
        [string]$Message
    )

    if (-not $Condition) {
        throw $Message
    }
}

function Wait-DriveBridgeActive {
    param([int]$TimeoutSeconds)

    $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    $lastInspect = $null
    while ($stopwatch.Elapsed.TotalSeconds -lt $TimeoutSeconds) {
        $lastInspect = Invoke-DriveJson -CommandArgs @("inspect")
        if ($lastInspect.bridge.active -eq $true) {
            return $lastInspect
        }
        Start-Sleep -Milliseconds 500
    }

    $reason = $lastInspect.bridge.reason
    if (-not $reason) {
        $reason = "bridge did not become active before timeout"
    }
    throw "drive bridge was not active: $reason"
}

function Stop-ProcessTree {
    param([int]$ProcessId)

    $children = Get-CimInstance Win32_Process -Filter "ParentProcessId=$ProcessId" -ErrorAction SilentlyContinue
    foreach ($child in $children) {
        Stop-ProcessTree -ProcessId $child.ProcessId
    }
    $process = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    if ($process) {
        Stop-Process -Id $ProcessId -Force -ErrorAction SilentlyContinue
    }
}

New-Item -ItemType Directory -Force $DataDir | Out-Null

Write-Host "Building CLI and dogfood web..."
cargo build -p auditaur-cli
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
if (-not (Test-Path (Join-Path $dogfood "node_modules"))) {
    Write-Host "Installing dogfood npm dependencies..."
    npm --prefix $dogfood ci
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
npm --prefix (Join-Path $repoRoot "packages\api") run build
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
npm --prefix $dogfood run build:web
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$env:AUDITAUR_DATA_DIR = $DataDir

$app = $null
try {
    Write-Host "Starting dogfood app with AUDITAUR_DATA_DIR=$DataDir..."
    $app = Start-Process -FilePath "npm.cmd" -ArgumentList @("run", "tauri", "dev") -WorkingDirectory $dogfood -PassThru

    Write-Host "Waiting for Auditaur readiness..."
    $watchArgs = @(
        "debug", "--app", "auditaur-dogfood", "--active", "--json", "watch",
        "--until-ready", "--timeout-seconds", "$TimeoutSeconds"
    )
    Invoke-JsonLineCommand -CommandArgs $watchArgs | Out-Null
    Write-Host "Inspecting Tauri-native drive bridge..."
    $inspect = Wait-DriveBridgeActive -TimeoutSeconds 30
    Assert-Condition ($inspect.bridge.targets.Count -gt 0) "drive bridge did not report a target"
    Assert-Condition ($inspect.platformBackend.selectorBackend -eq "tauri_in_app_driver") "drive did not report the Tauri-native selector backend"

    Invoke-DriveJson -CommandArgs @("wait", "--selector", "#successful-command", "--timeout-ms", "10000", "--visible-only") | Out-Null
    $exists = Invoke-DriveJson -CommandArgs @("exists", "--selector", "#drive-input", "--visible-only")
    Assert-Condition ($exists.payload.exists -eq $true) "drive exists did not find #drive-input"
    $heading = Invoke-DriveJson -CommandArgs @("text", "--selector", "h1", "--visible-only")
    Assert-Condition ($heading.payload.text -eq "Dogfood telemetry generator") "drive text returned an unexpected heading"

    $buttons = @(
        "console-log",
        "frontend-event",
        "successful-command",
        "failing-command",
        "backend-event",
        "frontend-error"
    )
    foreach ($button in $buttons) {
        Write-Host "Clicking #$button..."
        Invoke-DriveJson -CommandArgs @("click", "--selector", "#$button", "--visible-only") | Out-Null
        Start-Sleep -Milliseconds 250
    }

    Write-Host "Exercising bridge input actions..."
    Invoke-DriveJson -CommandArgs @("fill", "--selector", "#drive-input", "--value", "filled by dogfood smoke", "--visible-only") | Out-Null
    Invoke-DriveJson -CommandArgs @("press", "--selector", "#drive-input", "--key", "Enter") | Out-Null
    Invoke-DriveJson -CommandArgs @("type", "--selector", "#drive-textarea", "--value", " typed by dogfood smoke", "--visible-only") | Out-Null
    Invoke-DriveJson -CommandArgs @("select", "--selector", "#drive-select", "--value", "charlie", "--visible-only") | Out-Null
    Invoke-DriveJson -CommandArgs @("check", "--selector", "#drive-checkbox", "--visible-only") | Out-Null
    Invoke-DriveJson -CommandArgs @("uncheck", "--selector", "#drive-checkbox", "--visible-only") | Out-Null
    Invoke-DriveJson -CommandArgs @("hover", "--selector", "#drive-hover-target", "--visible-only") | Out-Null

    $evaluate = Invoke-DriveJson -CommandArgs @("evaluate", "--expression", "({input: document.querySelector('#drive-input')?.value, select: document.querySelector('#drive-select')?.value, checked: document.querySelector('#drive-checkbox')?.checked, output: document.querySelector('#output')?.textContent?.includes('Drive hover target hovered.')})")
    Assert-Condition ($evaluate.payload.value.input -eq "filled by dogfood smoke") "drive evaluate observed an unexpected input value"
    Assert-Condition ($evaluate.payload.value.select -eq "charlie") "drive evaluate observed an unexpected select value"
    Assert-Condition ($evaluate.payload.value.checked -eq $false) "drive evaluate observed an unexpected checkbox value"
    Assert-Condition ($evaluate.payload.value.output -eq $true) "drive evaluate did not observe the hover output"

    $artifactDir = Join-Path $DataDir "artifacts"
    New-Item -ItemType Directory -Force $artifactDir | Out-Null
    $snapshotPath = Join-Path $artifactDir "dogfood-snapshot.json"
    $screenshotPath = Join-Path $artifactDir "dogfood-screenshot.png"
    $screenshotSnapshotPath = Join-Path $artifactDir "dogfood-screenshot-snapshot.json"

    Write-Host "Capturing snapshot and native screenshot..."
    Invoke-DriveJson -CommandArgs @("snapshot", "--selector", "body", "--output", $snapshotPath) | Out-Null
    $screenshot = Invoke-DriveJson -CommandArgs @("screenshot", "--selector", "body", "--output", $screenshotPath, "--snapshot-output", $screenshotSnapshotPath)
    Assert-Condition (Test-Path $snapshotPath) "drive snapshot did not write $snapshotPath"
    Assert-Condition (Test-Path $screenshotPath) "drive screenshot did not write $screenshotPath"
    Assert-Condition (Test-Path $screenshotSnapshotPath) "drive screenshot did not write $screenshotSnapshotPath"
    Assert-Condition ($screenshot.payload.screenshotBackend -eq "tauri_native_window_xcap") "drive screenshot did not use native window capture: $($screenshot.payload.screenshotBackend); native error: $($screenshot.payload.nativeScreenshotError)"
    Assert-Condition ($screenshot.payload.width -gt 0 -and $screenshot.payload.height -gt 0) "drive screenshot reported invalid dimensions"

    Write-Host "Confirming frontend-required readiness..."
    $status = Invoke-JsonLineCommand -CommandArgs @(
        "debug", "--app", "auditaur-dogfood", "--active", "--require-frontend", "--json", "status"
    )
    if (-not $status.ready) {
        throw "dogfood app did not reach frontend-required readiness"
    }
    $dbPath = $status.databasePath
    if (-not $dbPath) {
        throw "debug status did not include databasePath"
    }

    Write-Host "Inspecting telemetry..."
    & $cli timeline --db $dbPath --json | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "timeline inspection failed" }
    & $cli explain --db $dbPath --json | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "explain inspection failed" }

    Write-Host "Dogfood smoke passed."
}
finally {
    if ($app -and -not $app.HasExited) {
        Write-Host "Stopping dogfood app process tree rooted at PID $($app.Id)..."
        Stop-ProcessTree -ProcessId $app.Id
    }
    if ($cleanupDataDir -and (Test-Path $DataDir)) {
        Remove-Item -Recurse -Force $DataDir -ErrorAction SilentlyContinue
    }
}
