[CmdletBinding()]
param(
    [ValidateSet('Debug', 'Release')]
    [string]$Configuration = 'Release',

    [ValidateSet('x64')]
    [string]$Architecture = 'x64',

    [string]$BuildDirectory = '',

    [ValidateSet('stable', 'preview')]
    [string]$UpdateChannel = 'stable',

    [switch]$RunSmokeTest
)

$ErrorActionPreference = 'Stop'

# MSBuild's .NET Framework host rejects environment blocks containing both
# PATH and Path. Normalize only this script process; user settings are untouched.
$processPath = $env:Path
[Environment]::SetEnvironmentVariable('PATH', $null, [EnvironmentVariableTarget]::Process)
[Environment]::SetEnvironmentVariable('Path', $processPath, [EnvironmentVariableTarget]::Process)

function Invoke-NativeCommand {
    param(
        [Parameter(Mandatory)]
        [string]$Executable,

        [Parameter(Mandatory)]
        [string[]]$Arguments
    )

    & $Executable @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed with exit code $LASTEXITCODE`: $Executable"
    }
}

$repositoryRoot = Split-Path -Parent $PSScriptRoot
$sourceDirectory = Join-Path $repositoryRoot 'native'

function Get-NativeSourceSnapshot {
    $entries = @(
        Get-ChildItem -LiteralPath $sourceDirectory -Recurse -File |
            Where-Object {
                $_.Name -eq 'CMakeLists.txt' -or
                $_.Extension -in @('.cpp', '.hpp', '.h', '.rc')
            } |
            Sort-Object FullName |
            ForEach-Object {
                $relativePath = $_.FullName.Substring($sourceDirectory.Length).TrimStart('\')
                $hash = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash
                "$relativePath=$hash"
            }
    )
    return [string]::Join("`n", $entries)
}

$sourceSnapshot = Get-NativeSourceSnapshot

if ([string]::IsNullOrWhiteSpace($BuildDirectory)) {
    $BuildDirectory = Join-Path $repositoryRoot 'build\native'
} else {
    $BuildDirectory = [System.IO.Path]::GetFullPath($BuildDirectory)
}

$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (-not (Test-Path -LiteralPath $vswhere)) {
    throw 'Visual Studio Installer (vswhere.exe) was not found.'
}

$instances = @(
    & $vswhere `
        -latest `
        -products '*' `
        -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
        -format json |
        ConvertFrom-Json
)

if ($instances.Count -eq 0) {
    throw 'Visual Studio with the Desktop C++ toolchain was not found.'
}

$instance = $instances[0]
$visualStudioPath = [string]$instance.installationPath
$visualStudioMajor = ([version]$instance.installationVersion).Major

$generator = switch ($visualStudioMajor) {
    18 { 'Visual Studio 18 2026' }
    17 { 'Visual Studio 17 2022' }
    default { throw "Unsupported Visual Studio major version: $visualStudioMajor" }
}

$bundledCMake = Join-Path $visualStudioPath 'Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe'
if (Test-Path -LiteralPath $bundledCMake) {
    $cmake = $bundledCMake
} else {
    $cmakeCommand = Get-Command cmake -ErrorAction SilentlyContinue
    if ($null -eq $cmakeCommand) {
        throw 'CMake was not found in Visual Studio or PATH.'
    }
    $cmake = $cmakeCommand.Source
}

Write-Host "Configuring native build with $generator ($Architecture)..."
$configureArguments = @(
    '-S', $sourceDirectory,
    '-B', $BuildDirectory,
    '-G', $generator,
    '-A', $Architecture,
    "-DSCREENTRANS_UPDATE_CHANNEL=$UpdateChannel"
)
Invoke-NativeCommand -Executable $cmake -Arguments $configureArguments

Write-Host "Building $Configuration..."
$buildArguments = @(
    '--build', $BuildDirectory,
    '--config', $Configuration,
    '--clean-first',
    '--parallel'
)
Invoke-NativeCommand -Executable $cmake -Arguments $buildArguments

if ($sourceSnapshot -ne (Get-NativeSourceSnapshot)) {
    throw 'Native sources changed while compilation was running. Discard this build and run the command again.'
}

$executable = Join-Path $BuildDirectory "$Configuration\ScreenTranslate.exe"
if (-not (Test-Path -LiteralPath $executable)) {
    throw "The expected executable was not produced: $executable"
}

if ($RunSmokeTest) {
    Write-Host 'Running native startup smoke test...'
    Invoke-NativeCommand -Executable $executable -Arguments @('--self-test')
    Write-Host 'Running native updater smoke test...'
    Invoke-NativeCommand -Executable $executable -Arguments @('--update-self-test')
}

Write-Host "Built: $executable"
