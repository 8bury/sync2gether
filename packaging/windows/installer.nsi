Unicode True
!include "MUI2.nsh"

Name "sync2gether ${VERSION}"
OutFile "${OUT_FILE}"
InstallDir "$LOCALAPPDATA\Programs\sync2gether"
InstallDirRegKey HKCU "Software\sync2gether" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma

!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\sync2gether.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Abrir o sync2gether"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "PortugueseBR"
!insertmacro MUI_LANGUAGE "English"

Section "sync2gether" SecMain
  SetOutPath "$INSTDIR"
  File "${STAGE_DIR}\sync2gether.exe"
  File "${STAGE_DIR}\libmpv-2.dll"
  File "${STAGE_DIR}\LICENSE"

  WriteUninstaller "$INSTDIR\uninstall.exe"

  CreateDirectory "$SMPROGRAMS\sync2gether"
  CreateShortcut "$SMPROGRAMS\sync2gether\sync2gether.lnk" "$INSTDIR\sync2gether.exe"
  CreateShortcut "$SMPROGRAMS\sync2gether\Desinstalar sync2gether.lnk" "$INSTDIR\uninstall.exe"

  WriteRegStr HKCU "Software\sync2gether" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\sync2gether" "DisplayName" "sync2gether"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\sync2gether" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\sync2gether" "Publisher" "8bury"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\sync2gether" "URLInfoAbout" "https://github.com/8bury/sync2gether"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\sync2gether" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\sync2gether" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\sync2gether" "NoRepair" 1
SectionEnd

Section "Uninstall"
  Delete "$SMPROGRAMS\sync2gether\sync2gether.lnk"
  Delete "$SMPROGRAMS\sync2gether\Desinstalar sync2gether.lnk"
  RMDir "$SMPROGRAMS\sync2gether"

  Delete "$INSTDIR\sync2gether.exe"
  Delete "$INSTDIR\libmpv-2.dll"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"

  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\sync2gether"
  DeleteRegKey HKCU "Software\sync2gether"
SectionEnd
