param(
    [int]$CdpPort = 9666,
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
npm --prefix (Join-Path $repoRoot "packages\api") run build
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
npm --prefix $dogfood run build:web
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$env:AUDITAUR_DATA_DIR = $DataDir
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$CdpPort"

$app = $null
try {
    Write-Host "Starting dogfood app with AUDITAUR_DATA_DIR=$DataDir and CDP port $CdpPort..."
    $app = Start-Process -FilePath "npm.cmd" -ArgumentList @("run", "tauri", "dev") -WorkingDirectory $dogfood -PassThru

    Write-Host "Waiting for Auditaur readiness..."
    $watchArgs = @(
        "debug", "--app", "auditaur-dogfood", "--active", "--cdp-port", "$CdpPort",
        "--json", "watch", "--until-ready", "--timeout-seconds", "$TimeoutSeconds"
    )
    Invoke-JsonLineCommand -CommandArgs $watchArgs | Out-Null

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
        & $cli drive --app auditaur-dogfood --active --cdp-port $CdpPort --json click --selector "#$button" --allow-unproven-target | Out-Null
        if ($LASTEXITCODE -ne 0) {
            throw "drive click #$button failed with exit code $LASTEXITCODE"
        }
        Start-Sleep -Milliseconds 250
    }

    Write-Host "Confirming frontend-required readiness..."
    $status = Invoke-JsonLineCommand -CommandArgs @(
        "debug", "--app", "auditaur-dogfood", "--active", "--cdp-port", "$CdpPort",
        "--require-frontend", "--json", "status"
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
