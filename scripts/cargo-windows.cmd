@echo off
setlocal
set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
if not exist "%VSWHERE%" (
  echo Install Visual Studio C++ Build Tools and the Windows SDK. 1>&2
  exit /b 1
)
for /f "usebackq delims=" %%i in (`"%VSWHERE%" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do set "OPENTRACK_VS=%%i"
if not defined OPENTRACK_VS (
  echo No installed Visual Studio C++ toolchain was found. 1>&2
  exit /b 1
)
call "%OPENTRACK_VS%\Common7\Tools\VsDevCmd.bat" -no_logo -arch=x64 -host_arch=x64
if errorlevel 1 exit /b %errorlevel%
cargo %*
exit /b %errorlevel%
