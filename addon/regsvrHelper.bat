@echo off
"%SystemRoot%\System32\regsvr32.exe" %3 /s "%1"
if %errorlevel% neq 0 exit /b %errorlevel%
"%SystemRoot%\SysWOW64\regsvr32.exe" %3 /s "%2"
if %errorlevel% neq 0 exit /b %errorlevel%
