# Build-Script für die BoxDoc Web-Version (Windows)
# ===================================================
# Baut die Web-Version und legt die PHP-Dateien dazu. Das Ergebnis liegt
# in 'dist/' — das ist der komplette Ordner für den Webserver.
#
# Aufruf:  .\build-web.ps1
# Danach:  Inhalt von dist/ auf den Webserver hochladen.

$ErrorActionPreference = "Stop"

Write-Host "=== 1/2 WASM bauen (trunk) ===" -ForegroundColor Cyan
trunk build --release
if ($LASTEXITCODE -ne 0) { Write-Host "Build fehlgeschlagen!" -ForegroundColor Red; exit 1 }

Write-Host "=== 2/2 PHP-Backend dazu kopieren ===" -ForegroundColor Cyan
Copy-Item web\api.php dist\ -Force
Copy-Item web\index.php dist\ -Force
Copy-Item web\stream.php dist\ -Force
Copy-Item web\.htaccess dist\ -Force
if (-not (Test-Path dist\docs)) { New-Item -ItemType Directory -Path dist\docs | Out-Null }
Copy-Item web\docs\.htaccess dist\docs\ -Force

Write-Host "=== Fertig ===" -ForegroundColor Green
Write-Host "dist/ enthält (hochzuladen):" -ForegroundColor Yellow
Get-ChildItem dist | Format-Table Name, Length -AutoSize
Write-Host "Inhalt von dist/ auf den Webserver laden." -ForegroundColor Green
Write-Host "docs/ muss für PHP schreibbar sein (chmod 0700)." -ForegroundColor Green
