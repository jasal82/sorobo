; Copyright (C) 2026 Johannes Asal
; SPDX-License-Identifier: AGPL-3.0-or-later
; See LICENSE in the project root for the full license and warranty disclaimer.

; Compile through scripts/build-installer.ps1 after building the release binaries.
#ifndef AppVersion
  #error AppVersion must be supplied by the build script
#endif

[Setup]
; Unchanged since the program was called Sqonto (until version 0.10): the setup updates such an
; installation in place, its folder (Programs\Sqonto) and the data in it stay where they are.
AppId={{01CF2607-5352-4334-AF58-D800028178A7}
AppName=Sorobo
AppVersion={#AppVersion}
AppPublisher=Sorobo
DefaultDirName={localappdata}\Programs\Sorobo
DefaultGroupName=Sorobo
; The start menu entry moves from "Sqonto" to "Sorobo" (the old one is removed below).
UsePreviousGroup=no
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
DisableProgramGroupPage=yes
LicenseFile=..\LICENSE
SetupIconFile=..\backend\assets\icons\sorobo.ico
UninstallDisplayIcon={app}\Sorobo.exe
OutputDir=..\dist\packages
OutputBaseFilename=Sorobo-{#AppVersion}-windows-x64-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "german"; MessagesFile: "compiler:Languages\German.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; Flags: unchecked

[InstallDelete]
; Programs and shortcuts from before the rename; the data folder is never touched.
Type: files; Name: "{app}\Sqonto.exe"
Type: files; Name: "{app}\sqonto-server.exe"
Type: files; Name: "{autoprograms}\Sqonto\Sqonto.lnk"
Type: dirifempty; Name: "{autoprograms}\Sqonto"
Type: files; Name: "{autodesktop}\Sqonto.lnk"

[Files]
; Explicit allowlist: never ship local accounting data or recursively uninstall it.
Source: "..\dist\Sorobo.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\dist\sorobo-server.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\dist\LICENSE.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\dist\THIRD-PARTY-LICENSES.txt"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Sorobo"; Filename: "{app}\Sorobo.exe"; WorkingDir: "{app}"
Name: "{autodesktop}\Sorobo"; Filename: "{app}\Sorobo.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{app}\Sorobo.exe"; Description: "{cm:LaunchProgram,Sorobo}"; Flags: nowait postinstall skipifsilent
; Self-update (backend/src/services/update.rs): /RELAUNCH=desktop|server starts the updated program again.
Filename: "{app}\Sorobo.exe"; Flags: nowait; Check: Relaunch('desktop')
Filename: "{app}\sorobo-server.exe"; Flags: nowait; Check: Relaunch('server')

[Code]
function Relaunch(Kind: String): Boolean;
begin
  Result := CompareText(ExpandConstant('{param:relaunch|}'), Kind) = 0;
end;
