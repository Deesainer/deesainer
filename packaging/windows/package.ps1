# Builds a portable zip for Windows into dist\.
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..\..")
$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches.Groups[1].Value
cargo build --release -p deesainer -p deesainer-cli -p deesainer-relay -p deesainer-teamserver
Remove-Item -Recurse -Force dist -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path dist\Deesainer | Out-Null
foreach ($bin in "deesainer", "deesainer-cli", "deesainer-relay", "deesainer-server") {
  Copy-Item "target\release\$bin.exe" dist\Deesainer\
}
Copy-Item assets\app-icon\deesainer.ico dist\Deesainer\
if ($env:WINDOWS_CERTIFICATE_PFX) {
  Write-Host "signing with the provided certificate"
  # signtool sign /f cert.pfx /p $env:WINDOWS_CERTIFICATE_PASSWORD /tr http://timestamp.digicert.com /td sha256 /fd sha256 dist\Deesainer\*.exe
} else {
  Write-Host "::warning::WINDOWS_CERTIFICATE_PFX not set; binaries are unsigned"
}
Compress-Archive -Path dist\Deesainer\* -DestinationPath "dist\Deesainer-$version-windows-x64.zip"
