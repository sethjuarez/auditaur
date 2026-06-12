param(
    [switch]$IncludeDogfoodSmoke,
    [switch]$AllowDirtyPackage,
    [switch]$SkipGhSkillDryRun
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot

function Invoke-Step {
    param(
        [string]$Name,
        [scriptblock]$Command
    )

    Write-Host ""
    Write-Host "==> $Name"
    & $Command
    if ($LASTEXITCODE -ne 0) {
        throw "$Name failed with exit code $LASTEXITCODE"
    }
}

Push-Location $repoRoot
try {
    Invoke-Step "Skill drift check" { python scripts\check-skill-drift.py }
    Invoke-Step "Rust formatting" { cargo fmt --check }
    Invoke-Step "Collector receiver compatibility tests" { cargo test -p auditaur-collector receiver }
    Invoke-Step "CLI tests" { cargo test -p auditaur-cli -- --test-threads=1 }
    Invoke-Step "API tests" { npm --prefix packages\api test }
    Invoke-Step "API build" { npm --prefix packages\api run build }
    Invoke-Step "Docs build" { npm --prefix docs run build }

    Invoke-Step "CLI crate package verification" {
        $args = @("package", "-p", "auditaur-cli")
        if ($AllowDirtyPackage) {
            $args += "--allow-dirty"
        }
        & cargo @args
    }

    if (-not $SkipGhSkillDryRun) {
        if (Get-Command gh -ErrorAction SilentlyContinue) {
            Invoke-Step "GitHub skill publish dry run" { gh skill publish .github --dry-run }
        } else {
            Write-Host "Skipping GitHub skill dry run because gh is not installed."
        }
    }

    if ($IncludeDogfoodSmoke) {
        Invoke-Step "Dogfood smoke" { powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\dogfood-smoke.ps1 }
    }

    Write-Host ""
    Write-Host "Release preflight passed."
}
finally {
    Pop-Location
}
