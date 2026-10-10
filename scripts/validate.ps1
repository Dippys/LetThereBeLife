# Full quality gate. Run from anywhere:
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts/validate.ps1 [-Quick] [-Gpu]
# -Quick skips the headless smoke run and the release success and slice tests. -Gpu adds the hidden-window viewer smoke.

param(
    [switch]$Quick,
    [switch]$Gpu
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Set-Location -LiteralPath (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path

function Invoke-Checked {
    param([string]$Name, [scriptblock]$Command)
    Write-Host "==> $Name"
    & $Command
    if ($LASTEXITCODE -ne 0) { throw "$Name failed with exit code $LASTEXITCODE" }
}

Write-Host '==> InitialDocumentation checksums'
$expected = Get-Content -LiteralPath 'scripts/initial-documentation.sha256' | Where-Object { $_.Trim() }
$actual = Get-ChildItem -LiteralPath 'InitialDocumentation' -File | Sort-Object Name | ForEach-Object {
    "$((Get-FileHash -Algorithm SHA256 -LiteralPath $_.FullName).Hash.ToLowerInvariant())  $($_.Name)"
}
if (Compare-Object -ReferenceObject $expected -DifferenceObject $actual) {
    throw 'InitialDocumentation/ was modified. It is read-only design input.'
}

Invoke-Checked 'cargo fmt --check' { cargo fmt --all -- --check }
Invoke-Checked 'cargo test' { cargo test --workspace }
Invoke-Checked 'cargo clippy' { cargo clippy --workspace --all-targets -- -D warnings }

if (-not $Quick) {
    Invoke-Checked 'Headless smoke' { cargo run -p sim-headless -- --ticks 600 --seed 42 }
    Invoke-Checked 'Definition of success and vertical slice (release)' { cargo test --release -p sim-headless --test success --test slice -- --ignored }
}
if ($Gpu) {
    Invoke-Checked 'Viewer GPU smoke' { cargo run -p sim-viewer -- --smoke-frames 2 }
}

Write-Host 'All validation stages passed.'
