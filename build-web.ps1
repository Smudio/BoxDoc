# Build-Script fuer die BoxDoc Web-Version (Windows)
# ==================================================
# trunk build kopiert ueber copy-file-Links in index.html AUTOMATISCH
# alle Dateien nach dist/: WASM + PHP-Backend + KI-Dateien + .htaccess.
#
# Aufruf:  .\build-web.ps1
# Danach:  Inhalt von dist/ auf den Webserver hochladen.

$ErrorActionPreference = "Stop"

Write-Host "=== Vorab: cargo check (WASM) ===" -ForegroundColor Cyan
cargo check --target wasm32-unknown-unknown
if ($LASTEXITCODE -ne 0) { Write-Host "cargo check fehlgeschlagen!" -ForegroundColor Red; exit 1 }

Write-Host "=== trunk build --release ===" -ForegroundColor Cyan
trunk build --release
if ($LASTEXITCODE -ne 0) { Write-Host "Build fehlgeschlagen!" -ForegroundColor Red; exit 1 }

Write-Host "=== Fertig — dist/ ist komplett ===" -ForegroundColor Green
Get-ChildItem dist | Format-Table Name, Length -AutoSize
Write-Host "Inhalt von dist/ auf den Webserver laden." -ForegroundColor Green
Write-Host "docs/ muss fuer PHP schreibbar sein (chmod 0700)." -ForegroundColor Green
