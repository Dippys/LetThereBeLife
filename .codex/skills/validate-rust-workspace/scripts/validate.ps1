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
Write-Host '==> Copied debug configuration'
$sourceConfig = 'config/simulation.toml'
$debugConfig = 'target/debug/config/simulation.toml'
if (-not (Test-Path -LiteralPath $debugConfig)) {
    throw "$debugConfig was not produced by the sim-config build"
}
if ((Get-FileHash -Algorithm SHA256 -LiteralPath $sourceConfig).Hash -ne
    (Get-FileHash -Algorithm SHA256 -LiteralPath $debugConfig).Hash) {
    throw "$debugConfig does not match $sourceConfig"
}
Invoke-Checked 'Clippy' { cargo clippy --workspace --all-targets -- -D warnings }
Invoke-Checked 'Headless smoke test' { cargo run -p sim-headless -- --ticks 600 --seed 42 }
Invoke-Checked 'GPU viewer smoke test' { cargo run -p sim-viewer -- --smoke-frames 2 }

if ($Runtime) {
    Write-Host 'Runtime flag selected. Manually verify the viewer because it is interactive:'
    Write-Host 'cargo run -p sim-viewer'
}

Write-Host 'All automated validation stages passed.'
