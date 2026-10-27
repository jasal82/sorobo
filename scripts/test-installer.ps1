# Copyright (C) 2026 Johannes Asal
# SPDX-License-Identifier: AGPL-3.0-or-later
# See LICENSE in the project root for the full license and warranty disclaimer.

# Exercise the actual generated installer in CI, including preservation of user data.
# -PreviousInstaller: setup of the last version from before the rename (Sqonto 0.10); its
# installation is then upgraded as well.
[CmdletBinding()]
param([string]$PreviousInstaller)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$installDir = Join-Path ([IO.Path]::GetTempPath()) ("sorobo-installer-test-" + [guid]::NewGuid())
$version = & node "$root/scripts/version.mjs"
if ($LASTEXITCODE -ne 0) { throw 'Version validation failed.' }
$installer = Join-Path $root "dist/packages/Sorobo-$version-windows-x64-setup.exe"
$server = $null
$saved = @{}
foreach ($name in 'SOROBO_PORT', 'SOROBO_DATA_DIR', 'SQONTO_PORT', 'SQONTO_DATA_DIR') {
    $saved[$name] = [Environment]::GetEnvironmentVariable($name)
}

# Waits until a server on $Port serves the data of $DataDir.
function Wait-Server([int]$Port, [string]$DataDir, $Process) {
    for ($attempt = 0; $attempt -lt 60; $attempt++) {
        if ($Process -and $Process.HasExited) { throw "Server on port $Port exited before becoming ready." }
        try {
            $status = Invoke-RestMethod "http://127.0.0.1:$Port/api/system/status" -TimeoutSec 2
            if ([IO.Path]::GetFullPath($status.data_dir) -eq [IO.Path]::GetFullPath($DataDir)) { return }
        } catch { }
        Start-Sleep -Seconds 1
    }
    throw "Server on port $Port did not serve $DataDir."
}

function Stop-Server($Process) {
    Stop-Process -Id $Process.Id
    $Process.WaitForExit()
}

try {
    $arguments = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/DIR=`"$installDir`"")
    $setup = Start-Process -FilePath $installer -ArgumentList $arguments -WindowStyle Hidden -PassThru -Wait
    if ($setup.ExitCode -ne 0) { throw "Installation failed: $($setup.ExitCode)" }
    foreach ($name in 'Sorobo.exe', 'sorobo-server.exe', 'LICENSE.txt', 'THIRD-PARTY-LICENSES.txt') {
        if (!(Test-Path "$installDir/$name")) { throw "Installer omitted $name" }
    }
    $env:SOROBO_DATA_DIR = $null
    $env:SQONTO_DATA_DIR = $null
    $env:SQONTO_PORT = $null
    $env:SOROBO_PORT = '18787'
    $server = Start-Process -FilePath "$installDir/sorobo-server.exe" -WindowStyle Hidden -PassThru
    $ready = $false
    for ($attempt = 0; $attempt -lt 60; $attempt++) {
        if ($server.HasExited) { throw 'Installed server exited before becoming ready.' }
        try {
            $status = Invoke-RestMethod 'http://127.0.0.1:18787/api/system/status' -TimeoutSec 2
            if ([IO.Path]::GetFullPath($status.data_dir) -ne [IO.Path]::GetFullPath("$installDir/data")) {
                throw 'Server reported an unexpected data directory.'
            }
            $page = Invoke-WebRequest 'http://127.0.0.1:18787/' -TimeoutSec 2
            if ($page.Content -notmatch '<html') { throw 'Embedded frontend missing.' }
            $ready = $true
            break
        } catch { Start-Sleep -Seconds 1 }
    }
    if (!$ready) { throw 'Installed server did not serve the API and frontend.' }
    Stop-Server $server
    $server = $null
    $marker = Join-Path $installDir 'data/preserve.txt'
    Set-Content -LiteralPath $marker -Value 'accounting data must survive'
    $setup = Start-Process -FilePath $installer -ArgumentList $arguments -WindowStyle Hidden -PassThru -Wait
    if ($setup.ExitCode -ne 0) { throw 'Upgrade failed.' }
    if (!(Test-Path $marker)) { throw 'Upgrade removed user data.' }
    # Self-update: the setup started by Sorobo starts the updated program again. Not with -Wait:
    # that also waits for all descendants, i.e. for the relaunched server to exit.
    $setup = Start-Process -FilePath $installer -ArgumentList ($arguments + '/RELAUNCH=server') -WindowStyle Hidden -PassThru
    $null = $setup.Handle # keeps the exit code readable after WaitForExit
    if (!$setup.WaitForExit(300000)) { throw 'Update with relaunch did not finish.' }
    if ($setup.ExitCode -ne 0) { throw "Update with relaunch failed: $($setup.ExitCode)" }
    $relaunched = $false
    for ($attempt = 0; $attempt -lt 60 -and !$relaunched; $attempt++) {
        try {
            $status = Invoke-RestMethod 'http://127.0.0.1:18787/api/system/status' -TimeoutSec 2
            $relaunched = [IO.Path]::GetFullPath($status.data_dir) -eq [IO.Path]::GetFullPath("$installDir/data")
        } catch { Start-Sleep -Seconds 1 }
    }
    # Found by its port: the temporary path may be given in its short 8.3 form.
    $owner = (Get-NetTCPConnection -LocalPort 18787 -State Listen -ErrorAction SilentlyContinue).OwningProcess | Select-Object -First 1
    if ($owner) {
        Stop-Process -Id $owner
        Wait-Process -Id $owner -ErrorAction SilentlyContinue
    }
    if (!$relaunched) { throw 'The setup did not start the server again after /RELAUNCH=server.' }
    $uninstall = Start-Process -FilePath "$installDir/unins000.exe" -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART' -WindowStyle Hidden -PassThru -Wait
    if ($uninstall.ExitCode -ne 0) { throw 'Uninstall failed.' }
    if (Test-Path "$installDir/Sorobo.exe") { throw 'Uninstall left the application installed.' }
    if (!(Test-Path $marker) -or !(Test-Path "$installDir/data/sorobo.db")) {
        throw 'Uninstall removed user data.'
    }
    Write-Host 'Installer, installed server, upgrade, relaunch and data preservation passed.'

    if ($PreviousInstaller) {
        # Upgrade of an installation made before the rename: same AppId, so the setup replaces
        # Sqonto in its folder and the database is taken over on the first start.
        $legacyDir = Join-Path ([IO.Path]::GetTempPath()) ("sorobo-upgrade-test-" + [guid]::NewGuid())
        $legacyArguments = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/DIR=`"$legacyDir`"")
        $setup = Start-Process -FilePath (Resolve-Path $PreviousInstaller) -ArgumentList $legacyArguments -WindowStyle Hidden -PassThru -Wait
        if ($setup.ExitCode -ne 0) { throw "Installation of the previous version failed: $($setup.ExitCode)" }
        # The previous version only knows the SQONTO_ variables; the new one still reads them.
        $env:SOROBO_PORT = $null
        $env:SQONTO_PORT = '18788'
        $server = Start-Process -FilePath "$legacyDir/sqonto-server.exe" -WindowStyle Hidden -PassThru
        Wait-Server 18788 "$legacyDir/data" $server
        Stop-Server $server
        $server = $null
        if (!(Test-Path "$legacyDir/data/sqonto.db")) { throw 'The previous version did not create its database.' }
        $legacyMarker = Join-Path $legacyDir 'data/preserve.txt'
        Set-Content -LiteralPath $legacyMarker -Value 'accounting data must survive the rename'

        $setup = Start-Process -FilePath $installer -ArgumentList $legacyArguments -WindowStyle Hidden -PassThru -Wait
        if ($setup.ExitCode -ne 0) { throw "Upgrade from the previous version failed: $($setup.ExitCode)" }
        foreach ($name in 'Sqonto.exe', 'sqonto-server.exe') {
            if (Test-Path "$legacyDir/$name") { throw "Upgrade left $name behind." }
        }
        foreach ($name in 'Sorobo.exe', 'sorobo-server.exe') {
            if (!(Test-Path "$legacyDir/$name")) { throw "Upgrade did not install $name." }
        }
        $server = Start-Process -FilePath "$legacyDir/sorobo-server.exe" -WindowStyle Hidden -PassThru
        Wait-Server 18788 "$legacyDir/data" $server
        Stop-Server $server
        $server = $null
        if (!(Test-Path "$legacyDir/data/sorobo.db") -or (Test-Path "$legacyDir/data/sqonto.db")) {
            throw 'The database of the previous version was not taken over.'
        }
        if (!(Test-Path $legacyMarker)) { throw 'Upgrade from the previous version removed user data.' }
        $uninstall = Start-Process -FilePath "$legacyDir/unins000.exe" -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART' -WindowStyle Hidden -PassThru -Wait
        if ($uninstall.ExitCode -ne 0) { throw 'Uninstall after the upgrade failed.' }
        if (!(Test-Path "$legacyDir/data/sorobo.db")) { throw 'Uninstall after the upgrade removed user data.' }
        Write-Host 'Upgrade of an installation from before the rename passed.'
    }
} finally {
    if ($server -and !$server.HasExited) { Stop-Process -Id $server.Id }
    foreach ($name in $saved.Keys) { [Environment]::SetEnvironmentVariable($name, $saved[$name]) }
    # Retain the temporary test data for inspection; never delete accounting data recursively.
}
