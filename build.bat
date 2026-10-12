@echo off
REM Build release memakai MSVC dari VS 2022 BuildTools.
REM (VS 18 di mesin ini tidak punya lib x64 lengkap -> error LNK1104 msvcrt.lib)
setlocal
set "VCVARS=C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
if not exist "%VCVARS%" goto novc
call "%VCVARS%" >nul
cd /d "%~dp0"
cargo build --release %*
if errorlevel 1 exit /b 1
echo.
echo Binary: %~dp0target\release\cachechecker.exe
exit /b 0

:novc
echo vcvars64.bat tidak ditemukan: "%VCVARS%"
exit /b 1
