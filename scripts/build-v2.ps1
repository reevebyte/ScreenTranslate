[CmdletBinding()]
param(
    [switch]$Debug,
    [switch]$NoBundle
)

$ErrorActionPreference = 'Stop'
$repository = Split-Path -Parent $PSScriptRoot
Push-Location $repository
try {
    pnpm.cmd install --frozen-lockfile
    if ($LASTEXITCODE -ne 0) { throw 'Dependency installation failed.' }
    pnpm.cmd types:check
    if ($LASTEXITCODE -ne 0) { throw 'Generated types are out of date.' }
    pnpm.cmd check
    if ($LASTEXITCODE -ne 0) { throw 'Svelte checks failed.' }
    pnpm.cmd test
    if ($LASTEXITCODE -ne 0) { throw 'Frontend tests failed.' }
    cargo test --workspace --locked
    if ($LASTEXITCODE -ne 0) { throw 'Rust tests failed.' }
    $arguments = @('--filter', '@screentranslate/desktop', 'tauri', 'build')
    if ($Debug) { $arguments += '--debug' }
    if ($NoBundle) { $arguments += '--no-bundle' }
    pnpm.cmd @arguments
    if ($LASTEXITCODE -ne 0) { throw 'Desktop build failed.' }
    node scripts/measure-v2.mjs
    if ($LASTEXITCODE -ne 0) { throw 'Frontend bundle budget exceeded.' }
} finally {
    Pop-Location
}
