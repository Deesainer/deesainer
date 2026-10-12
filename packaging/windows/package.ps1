# Builds a portable zip and, when makensis is on PATH, an NSIS installer into dist\.
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..\..")
$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches.Groups[1].Value
cargo build --release -p deesainer -p deesainer-cli -p deesainer-relay -p deesainer-teamserver
Remove-Item -Recurse -Force dist -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path dist\Deesainer | Out-Null
New-Item -ItemType Directory -Path dist\installer-stage | Out-Null
foreach ($bin in "deesainer", "deesainer-cli", "deesainer-relay", "deesainer-server") {
  Copy-Item "target\release\$bin.exe" dist\Deesainer\
  Copy-Item "target\release\$bin.exe" dist\installer-stage\
}
Copy-Item assets\app-icon\deesainer.ico dist\Deesainer\
Copy-Item assets\app-icon\deesainer.ico dist\installer-stage\
$signtool = $null
$pfx = $null
if ($env:WINDOWS_CERTIFICATE_PFX) {
  $pfx = Join-Path $env:TEMP "deesainer-cert.pfx"
  [IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:WINDOWS_CERTIFICATE_PFX))
  $signtool = Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\bin\10.0.*\x64\signtool.exe" -ErrorAction SilentlyContinue |
    Sort-Object FullName -Descending | Select-Object -First 1
  if ($signtool) {
    Get-ChildItem dist\installer-stage\*.exe | ForEach-Object {
      & $signtool.FullName sign /f $pfx /p $env:WINDOWS_CERTIFICATE_PASSWORD /tr http://timestamp.digicert.com /td sha256 /fd sha256 $_.FullName
      if ($LASTEXITCODE -ne 0) { throw "signtool failed on $($_.Name)" }
    }
    Copy-Item dist\installer-stage\*.exe dist\Deesainer\ -Force
  } else {
    Write-Host "::warning::signtool not found; binaries are unsigned"
  }
} else {
  Write-Host "::warning::WINDOWS_CERTIFICATE_PFX not set; binaries are unsigned"
}
$makensis = Get-Command makensis -ErrorAction SilentlyContinue
if ($makensis) {
  & makensis /DVERSION=$version packaging\windows\deesainer.nsi
  if ($LASTEXITCODE -ne 0) { throw "makensis failed" }
  if ($signtool) {
    & $signtool.FullName sign /f $pfx /p $env:WINDOWS_CERTIFICATE_PASSWORD /tr http://timestamp.digicert.com /td sha256 /fd sha256 "dist\Deesainer-$version-windows-x64.exe"
    if ($LASTEXITCODE -ne 0) { throw "signtool failed on the installer" }
  }
} else {
  Write-Host "::warning::makensis not found; installer skipped"
}
if ($pfx) {
  Remove-Item $pfx -Force
}
Compress-Archive -Path dist\Deesainer\* -DestinationPath "dist\Deesainer-$version-windows-x64.zip"
