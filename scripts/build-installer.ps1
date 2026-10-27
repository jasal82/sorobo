# Copyright (C) 2026 Johannes Asal
# SPDX-License-Identifier: AGPL-3.0-or-later
# See LICENSE in the project root for the full license and warranty disclaimer.

[CmdletBinding()]
param([string]$Iscc = $env:ISCC)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Push-Location $root
try {
    $version = & node scripts/version.mjs
    if ($LASTEXITCODE -ne 0) { throw 'Version validation failed.' }
    $hostInfo = & rustc -vV
    if ($LASTEXITCODE -ne 0 -or $hostInfo -notcontains 'host: x86_64-pc-windows-msvc') {
        throw 'The Windows installer requires an x64 MSVC Rust toolchain.'
    }
    if (!$Iscc) {
        $command = Get-Command ISCC.exe -ErrorAction SilentlyContinue
        if ($command) { $Iscc = $command.Source }
        else { $Iscc = Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6/ISCC.exe' }
    }
    if (!(Test-Path -LiteralPath $Iscc)) {
        throw 'Install Inno Setup 6 (https://jrsoftware.org/isdl.php), add ISCC.exe to PATH or set ISCC to its full path. For portable binaries only, use build.cmd --no-installer.'
    }
    foreach ($file in 'Sorobo.exe', 'sorobo-server.exe', 'LICENSE.txt', 'THIRD-PARTY-LICENSES.txt') {
        if (!(Test-Path -LiteralPath "dist/$file")) { throw "Missing dist/$file. Run build.cmd first." }
    }
    & $Iscc "/DAppVersion=$version" 'installer/sorobo.iss'
    if ($LASTEXITCODE -ne 0) { throw 'Installer compilation failed.' }
} finally { Pop-Location }
