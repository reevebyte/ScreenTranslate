[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$Path,

    [string]$ExpectedVersion = '',

    [switch]$SkipDependencyCheck
)

$ErrorActionPreference = 'Stop'

$executable = [System.IO.Path]::GetFullPath($Path)
if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
    throw "Native executable was not found: $executable"
}

$file = Get-Item -LiteralPath $executable
if ($file.Length -le 0 -or $file.Length -gt 512MB) {
    throw "Native executable has an invalid size: $($file.Length) bytes"
}

$bytes = [System.IO.File]::ReadAllBytes($executable)
if ($bytes.Length -lt 256 -or $bytes[0] -ne 0x4D -or $bytes[1] -ne 0x5A) {
    throw 'Native executable is not a valid PE file (missing MZ header)'
}
$peOffset = [BitConverter]::ToInt32($bytes, 0x3C)
if ($peOffset -lt 0x40 -or $peOffset + 96 -gt $bytes.Length) {
    throw 'Native executable has an invalid PE header offset'
}
if ([BitConverter]::ToUInt32($bytes, $peOffset) -ne 0x00004550) {
    throw 'Native executable is not a valid PE file (missing PE signature)'
}
if ([BitConverter]::ToUInt16($bytes, $peOffset + 4) -ne 0x8664) {
    throw 'Native executable is not an AMD64 binary'
}
$optionalHeader = $peOffset + 24
if ([BitConverter]::ToUInt16($bytes, $optionalHeader) -ne 0x020B) {
    throw 'Native executable does not use the PE32+ format'
}
if ([BitConverter]::ToUInt16($bytes, $optionalHeader + 68) -ne 2) {
    throw 'Native executable is not a Windows GUI application'
}

$versionInfo = [System.Diagnostics.FileVersionInfo]::GetVersionInfo($executable)
if ([string]::IsNullOrWhiteSpace($versionInfo.FileVersion)) {
    throw 'Native executable has no Win32 version resource'
}

if (-not [string]::IsNullOrWhiteSpace($ExpectedVersion)) {
    $match = [regex]::Match(
        $ExpectedVersion,
        '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$'
    )
    if (-not $match.Success) {
        throw "ExpectedVersion is not SemVer: $ExpectedVersion"
    }
    $actual = '{0}.{1}.{2}.{3}' -f $versionInfo.FileMajorPart, $versionInfo.FileMinorPart,
        $versionInfo.FileBuildPart, $versionInfo.FilePrivatePart
    $expectedNumeric = '{0}.{1}.{2}.0' -f $match.Groups[1].Value, $match.Groups[2].Value,
        $match.Groups[3].Value
    if ($actual -cne $expectedNumeric) {
        throw "Native executable version is $actual, expected $expectedNumeric"
    }
}

$adjacentDlls = @(Get-ChildItem -LiteralPath $file.DirectoryName -Filter '*.dll' -File)
if ($adjacentDlls.Count -ne 0) {
    throw "Native release is not self-contained; adjacent DLLs were found: $($adjacentDlls.Name -join ', ')"
}

if (-not $SkipDependencyCheck) {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path -LiteralPath $vswhere)) {
        throw 'Visual Studio Installer (vswhere.exe) was not found for dependency inspection'
    }
    $visualStudioPath = & $vswhere -latest -products '*' `
        -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
        -property installationPath
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($visualStudioPath)) {
        throw 'Visual Studio C++ tools were not found for dependency inspection'
    }
    $dumpbin = Get-ChildItem -Path (Join-Path $visualStudioPath 'VC\Tools\MSVC') `
        -Filter dumpbin.exe -File -Recurse |
        Where-Object { $_.FullName -like '*\bin\Hostx64\x64\dumpbin.exe' } |
        Sort-Object FullName -Descending |
        Select-Object -First 1
    if ($null -eq $dumpbin) {
        throw 'dumpbin.exe was not found in the Visual Studio C++ tools'
    }

    $dependencies = & $dumpbin.FullName /nologo /dependents $executable 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "dumpbin dependency inspection failed with exit code $LASTEXITCODE"
    }
    $forbidden = @(
        $dependencies | Select-String -Pattern `
            '(?i)\b(?:VCRUNTIME[0-9_]*|MSVCP[0-9_]*|CONCRT[0-9_]*|UCRTBASE|api-ms-win-crt-[a-z0-9-]+|python[0-9]*|Qt[0-9]*Core)\.dll\b'
    )
    if ($forbidden.Count -ne 0) {
        throw "Native release imports a forbidden runtime DLL: $($forbidden.Line.Trim() -join ', ')"
    }
}

$signature = Get-AuthenticodeSignature -LiteralPath $executable
Write-Host "[native] AMD64 PE32+ GUI executable: $executable"
Write-Host "[native] Size: $($file.Length) bytes; file version: $($versionInfo.FileVersion)"
Write-Host "[native] Forbidden runtime DLLs: none; Authenticode: $($signature.Status)"
