@echo off
rem Trust the wireproxy-control local CA for local HTTPS domains.
rem Chrome/Edge on Windows only trust CAs installed to LocalMachine\Root,
rem so this needs administrator rights (UAC prompt).
if not exist "%~dp0certs\ca.crt" (
  echo CA certificate not found. Start the stack once so certs\ca.crt is generated.
  pause
  exit /b 1
)
net session >nul 2>&1
if errorlevel 1 (
  echo Requesting administrator rights...
  powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
  exit /b
)
certutil -addstore -f Root "%~dp0certs\ca.crt"
if errorlevel 1 (
  echo Failed to add the CA to LocalMachine Root.
  pause
  exit /b 1
)
rem дублируем в пользовательское хранилище: его используют curl и часть клиентов
certutil -addstore -user Root "%~dp0certs\ca.crt" >nul 2>&1
echo.
echo CA added to LocalMachine Root. Chrome/Edge now trust local HTTPS domains.
echo Firefox: about:config -^> security.enterprise_roots.enabled = true
echo curl (schannel): add --ssl-no-revoke if it complains about revocation.
echo To remove:  certutil -delstore Root "wireproxy-control local CA"
echo              certutil -delstore -user Root "wireproxy-control local CA"
pause
