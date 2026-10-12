; Deesainer Windows installer. Compiled by packaging/windows/package.ps1.
Unicode true
!include "MUI2.nsh"

!ifndef VERSION
!define VERSION "dev"
!endif

!define APPNAME "Deesainer"
!define MUI_ICON "..\..\assets\app-icon\deesainer.ico"
!define MUI_UNICON "..\..\assets\app-icon\deesainer.ico"

Name "${APPNAME}"
OutFile "..\..\dist\Deesainer-${VERSION}-windows-x64.exe"
InstallDir "$LOCALAPPDATA\Programs\Deesainer"
RequestExecutionLevel user
SetCompressor /SOLID lzma

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "..\..\LICENSE-MIT"
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

Section "Deesainer" SecMain
  SectionIn RO
  SetOutPath "$INSTDIR"
  File "..\..\dist\installer-stage\deesainer.exe"
  File "..\..\dist\installer-stage\deesainer-cli.exe"
  File "..\..\dist\installer-stage\deesainer-relay.exe"
  File "..\..\dist\installer-stage\deesainer-server.exe"
  File "..\..\dist\installer-stage\deesainer.ico"

  CreateDirectory "$SMPROGRAMS\${APPNAME}"
  CreateShortCut "$SMPROGRAMS\${APPNAME}\Deesainer.lnk" "$INSTDIR\deesainer.exe"

  WriteUninstaller "$INSTDIR\Uninstall.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}" "DisplayName" "${APPNAME}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}" "Publisher" "Deesainer contributors"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}" "DisplayIcon" "$INSTDIR\deesainer.ico"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}" "InstallLocation" "$INSTDIR"
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}" "NoRepair" 1

  WriteRegStr HKCU "Software\Classes\.deesainer" "" "Deesainer.Document"
  WriteRegStr HKCU "Software\Classes\Deesainer.Document" "" "Deesainer document"
  WriteRegStr HKCU "Software\Classes\Deesainer.Document\DefaultIcon" "" "$INSTDIR\deesainer.ico"
  WriteRegStr HKCU "Software\Classes\Deesainer.Document\shell\open\command" "" '"$INSTDIR\deesainer.exe" "%1"'
  WriteRegStr HKCU "Software\Classes\Deesainer.Document\shell\open" "FriendlyAppName" "Deesainer"
SectionEnd

Section "Open .fig files with Deesainer" SecFig
  IfSilent fig_done
  WriteRegStr HKCU "Software\Classes\.fig" "" "Deesainer.Fig"
  WriteRegStr HKCU "Software\Classes\Deesainer.Fig" "" "Figma document"
  WriteRegStr HKCU "Software\Classes\Deesainer.Fig\DefaultIcon" "" "$INSTDIR\deesainer.ico"
  WriteRegStr HKCU "Software\Classes\Deesainer.Fig\shell\open\command" "" '"$INSTDIR\deesainer.exe" "%1"'
  fig_done:
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecMain} "The Deesainer app, the command line tools, the local relay and the team server."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecFig} "Optional: make Deesainer the default app for .fig files. Leave off to keep the current association."
!insertmacro MUI_FUNCTION_DESCRIPTION_END

Section "Uninstall"
  Delete "$INSTDIR\deesainer.exe"
  Delete "$INSTDIR\deesainer-cli.exe"
  Delete "$INSTDIR\deesainer-relay.exe"
  Delete "$INSTDIR\deesainer-server.exe"
  Delete "$INSTDIR\deesainer.ico"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\${APPNAME}\Deesainer.lnk"
  RMDir "$SMPROGRAMS\${APPNAME}"

  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}"

  ReadRegStr $0 HKCU "Software\Classes\.deesainer" ""
  StrCmp $0 "Deesainer.Document" 0 +2
  DeleteRegKey HKCU "Software\Classes\.deesainer"
  DeleteRegKey HKCU "Software\Classes\Deesainer.Document"

  ReadRegStr $0 HKCU "Software\Classes\.fig" ""
  StrCmp $0 "Deesainer.Fig" 0 +2
  DeleteRegKey HKCU "Software\Classes\.fig"
  DeleteRegKey HKCU "Software\Classes\Deesainer.Fig"
SectionEnd
