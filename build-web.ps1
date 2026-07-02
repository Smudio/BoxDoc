# Build-Script für die BoxDoc Web-Version (Windows)
# ==================================================
# trunk build kopiert über copy-file-Links in index.html AUTOMATISCH
# alle Dateien nach dist/: WASM + PHP-Backend + KI-Dateien + .htaccess.
#
# Aufruf:  .\build-web.ps1
# Danach:  Inhalt von dist/ auf den Webserver hochladen.

$ErrorActionPreference = "Stop"

Write-Host "=== trunk build --release ===" -ForegroundColor Cyan
trunk build --release
if ($LASTEXITCODE -ne 0) { Write-Host "Build fehlgeschlagen!" -ForegroundColor Red; exit 1 }

Write-Host "=== Fertig — dist/ ist komplett ===" -ForegroundColor Green
Get-ChildItem dist | Format-Table Name, Length -AutoSize
Write-Host "Inhalt von dist/ auf den Webserver laden." -ForegroundColor Green
Write-Host "docs/ muss fuer PHP schreibbar sein (chmod 0700)." -ForegroundColor Green
