$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $root
$venv = Join-Path $root '.build-venv'
if (-not (Test-Path $venv)) {
    python -m venv $venv
}
$py = Join-Path $venv 'Scripts\python.exe'
& $py -m pip install --upgrade pip
& $py -m pip install -r requirements-build.txt
& $py -m PyInstaller --noconfirm --clean tgws.spec
Write-Host "Sidecar built: $root\dist\tgws-proxy.exe"
