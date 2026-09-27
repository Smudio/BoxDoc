# Build-Script fuer die BoxDoc Web-Version (Windows)
# ==================================================
# trunk build kopiert ueber copy-file-Links in index.html AUTOMATISCH
# alle Dateien nach web/dist/: WASM + PHP-Backend + KI-Dateien + .htaccess.
#
# Aufruf:  .\build-web.ps1
# Danach:  Inhalt von web/dist/ auf den Webserver hochladen.
#
# WICHTIG: web/ ist die Quelle, web/dist/ das Ergebnis. `trunk build` schreibt
# web/dist/ komplett neu — dort editieren ist verloren. Siehe web/README.md.

$ErrorActionPreference = "Stop"

Write-Host "=== Vorab: cargo check (WASM) ===" -ForegroundColor Cyan
cargo check --target wasm32-unknown-unknown --bin boxdoc
if ($LASTEXITCODE -ne 0) { Write-Host "cargo check fehlgeschlagen!" -ForegroundColor Red; exit 1 }

# --cargo-profile wasm-release: das Profil aus Cargo.toml (opt-level="s", lto,
# panic="abort"). Ohne das nimmt trunk das normale release-Profil und das WASM
# ist rund 1,8 MB groesser (7,5 statt 5,7 MB). Absichtlich NICHT in Trunk.toml,
# denn dann wuerde `trunk serve` beim Entwickeln auch damit bauen — zwei Minuten
# pro Rebuild statt ein paar Sekunden.
Write-Host "=== trunk build --release (Profil: wasm-release) ===" -ForegroundColor Cyan
trunk build --release --cargo-profile wasm-release
if ($LASTEXITCODE -ne 0) { Write-Host "Build fehlgeschlagen!" -ForegroundColor Red; exit 1 }

Write-Host "=== Fertig - web/dist/ ist komplett ===" -ForegroundColor Green
Get-ChildItem web/dist | Format-Table Name, Length -AutoSize
Write-Host "Inhalt von web/dist/ auf den Webserver laden." -ForegroundColor Green
Write-Host "docs/ muss fuer PHP schreibbar sein (chmod 0700)." -ForegroundColor Green
