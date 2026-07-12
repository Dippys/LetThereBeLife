param(
    [switch]$Runtime
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$root = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..\..\..')).Path
Set-Location -LiteralPath $root

function Invoke-Checked {
    param([string]$Name, [scriptblock]$Command)
    Write-Host "==> $Name"
    & $Command
    if ($LASTEXITCODE -ne 0) {
        throw "$Name failed with exit code $LASTEXITCODE"
    }
}

Write-Host '==> Immutable initial documentation'
$manifestPath = '.codex/initial-documentation.sha256'
$expected = Get-Content -LiteralPath $manifestPath | Where-Object { $_.Trim() }
$actual = Get-ChildItem -LiteralPath 'InitialDocumentation' -File | Sort-Object Name | ForEach-Object {
    $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $_.FullName).Hash.ToLowerInvariant()
    "$hash  $($_.Name)"
}
if (Compare-Object -ReferenceObject $expected -DifferenceObject $actual) {
    throw 'InitialDocumentation differs from its immutable checksum manifest.'
}

Write-Host '==> Repository skills'
$codexHome = if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path $HOME '.codex' }
$validator = Join-Path $codexHome 'skills\.system\skill-creator\scripts\quick_validate.py'
if (-not (Test-Path -LiteralPath $validator)) {
    throw "Skill validator not found at $validator"
}
Get-ChildItem -LiteralPath '.codex/skills' -Directory | ForEach-Object {
    uv run --with pyyaml python $validator $_.FullName
    if ($LASTEXITCODE -ne 0) { throw "Skill validation failed: $($_.Name)" }
}

Invoke-Checked 'Cargo formatting' { cargo fmt --all -- --check }
Invoke-Checked 'Workspace tests' { cargo test --workspace }
Invoke-Checked 'Clippy' { cargo clippy --workspace --all-targets -- -D warnings }
Invoke-Checked 'Headless smoke test' { cargo run -p sim-server -- --ticks 600 --seed 42 }

if ($Runtime) {
    Write-Host 'Runtime flag selected. Manually verify the viewer because it is interactive:'
    Write-Host 'cargo run -p sim-viewer'
}

Write-Host 'All automated validation stages passed.'
