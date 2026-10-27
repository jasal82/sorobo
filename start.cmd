@echo off
rem Copyright (C) 2026 Johannes Asal
rem SPDX-License-Identifier: AGPL-3.0-or-later
rem See LICENSE in the project root for the full license and warranty disclaimer.

rem Startet Sorobo. Baut die Anwendung beim ersten Aufruf.
setlocal
cd /d "%~dp0"
if not exist dist\Sorobo.exe (
  call build.cmd || exit /b 1
)
start "Sorobo" dist\Sorobo.exe
