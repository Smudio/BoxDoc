# Build-Script für die BoxDoc Web-Version (Windows)
# ===================================================
# Erzeugt einen einzigen Ordner 'deploy/', der alles enthält was der
# Webserver braucht: WASM, index.html, api.php, .htaccess, docs/.
#
# Aufruf:  .\build-web.ps1
# Danach:  Inhalt von deploy/ auf den Webserver hochladen.

$ErrorActionPreference = "Stop"

Write-Host "=== 1/3 WASM bauen (trunk) ===" -ForegroundColor Cyan
trunk build --release
if ($LASTEXITCODE -ne 0) { Write-Host "Build fehlgeschlagen!" -ForegroundColor Red; exit 1 }

Write-Host "=== 2/3 deploy/ Ordner vorbereiten ===" -ForegroundColor Cyan
if (Test-Path deploy) { Remove-Item -Recurse -Force deploy }
New-Item -ItemType Directory -Path deploy | Out-Null
New-Item -ItemType Directory -Path deploy\docs | Out-Null

# WASM-Build aus dist/
Copy-Item dist\* deploy\ -Recurse

# PHP-Backend aus web/
Copy-Item web\api.php deploy\
Copy-Item web\index.php deploy\
Copy-Item web\stream.php deploy\
Copy-Item web\.htaccess deploy\ -Force

Write-Host "=== 3/3 Fertig ===" -ForegroundColor Green
Write-Host "deploy/ enthält:" -ForegroundColor Yellow
Get-ChildItem deploy | Format-Table Name, Length -AutoSize
Write-Host ""
Write-Host "Inhalt von deploy/ auf den Webserver laden." -ForegroundColor Green
Write-Host "Der docs/ Ordner muss für PHP schreibbar sein (chmod 0700)." -ForegroundColor Green
