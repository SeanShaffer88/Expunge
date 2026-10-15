; Expunge installer (NSIS 3, Unicode)
; Copyright (C) 2026 Sean Shaffer — GPLv3
;
; Build:  makensis -DVERSION=0.1.0 -DEXE=path\to\Expunge.exe installer\expunge.nsi
; Output: Expunge-Setup-<VERSION>.exe next to this script (or -DOUTDIR=...)

Unicode true
ManifestDPIAware true
RequestExecutionLevel admin
SetCompressor /SOLID lzma

!ifndef VERSION
  !define VERSION "0.0.0"
!endif
!ifndef EXE
  !define EXE "..\target\release\expunge.exe"
!endif
!ifndef OUTDIR
  !define OUTDIR "."
!endif

!define APP "Expunge"
!define PUBLISHER "Sean Shaffer"
!define URL "https://github.com/SeanShaffer88/ExpungeUninstaller"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Expunge"

Name "${APP} ${VERSION}"
OutFile "${OUTDIR}\Expunge-Setup-${VERSION}.exe"
InstallDir "$PROGRAMFILES64\${APP}"
InstallDirRegKey HKLM "${UNINST_KEY}" "InstallLocation"
BrandingText "${APP} ${VERSION} · © ${PUBLISHER}"

VIProductVersion "${VERSION}.0"
VIAddVersionKey /LANG=1033 "ProductName" "${APP}"
VIAddVersionKey /LANG=1033 "CompanyName" "${PUBLISHER}"
VIAddVersionKey /LANG=1033 "LegalCopyright" "© 2026 ${PUBLISHER} (GPLv3)"
VIAddVersionKey /LANG=1033 "FileDescription" "${APP} Setup"
VIAddVersionKey /LANG=1033 "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=1033 "ProductVersion" "${VERSION}"

!include "MUI2.nsh"
!include "x64.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "WinVer.nsh"

!macro AddShellVerb EXT
  WriteRegStr HKLM "Software\Classes\SystemFileAssociations\${EXT}\shell\ExpungeInstall" "MUIVerb" "Install with Expunge"
  WriteRegStr HKLM "Software\Classes\SystemFileAssociations\${EXT}\shell\ExpungeInstall" "Icon" '"$INSTDIR\Expunge.exe",0'
  WriteRegStr HKLM "Software\Classes\SystemFileAssociations\${EXT}\shell\ExpungeInstall\command" "" '"$INSTDIR\Expunge.exe" --monitor-install "%1"'
!macroend

!define MUI_ICON "..\assets\expunge.ico"
!define MUI_UNICON "..\assets\expunge.ico"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\Expunge.exe"
!define MUI_FINISHPAGE_LINK "${URL}"
!define MUI_FINISHPAGE_LINK_LOCATION "${URL}"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "..\LICENSE"
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

; Same languages as the app itself.
!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "Spanish"
!insertmacro MUI_LANGUAGE "German"
!insertmacro MUI_LANGUAGE "French"
!insertmacro MUI_LANGUAGE "PortugueseBR"
!insertmacro MUI_LANGUAGE "Italian"
!insertmacro MUI_LANGUAGE "Russian"
!insertmacro MUI_LANGUAGE "SimpChinese"
!insertmacro MUI_LANGUAGE "Japanese"
!insertmacro MUI_LANGUAGE "Korean"
!insertmacro MUI_LANGUAGE "Polish"
!insertmacro MUI_RESERVEFILE_LANGDLL

Function .onInit
  ; Expunge is built for Windows 11 only (build 22000 or later, 64-bit).
  ${IfNot} ${RunningX64}
  ${OrIfNot} ${AtLeastBuild} 22000
    MessageBox MB_ICONSTOP "Expunge requires 64-bit Windows 11."
    Abort
  ${EndIf}
  SetRegView 64
  !insertmacro MUI_LANGDLL_DISPLAY
FunctionEnd

Function un.onInit
  SetRegView 64
FunctionEnd

Section "Expunge" SecMain
  SectionIn RO
  SetOutPath "$INSTDIR"
  File "/oname=Expunge.exe" "${EXE}"
  File "/oname=LICENSE.txt" "..\LICENSE"
  WriteUninstaller "$INSTDIR\Uninstall.exe"

  ; Start menu
  CreateShortcut "$SMPROGRAMS\${APP}.lnk" "$INSTDIR\Expunge.exe"

  ; Apps & features entry
  WriteRegStr HKLM "${UNINST_KEY}" "DisplayName" "${APP}"
  WriteRegStr HKLM "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "${UNINST_KEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKLM "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\Expunge.exe,0"
  WriteRegStr HKLM "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKLM "${UNINST_KEY}" "URLInfoAbout" "${URL}"
  WriteRegStr HKLM "${UNINST_KEY}" "HelpLink" "${URL}/issues"
  WriteRegStr HKLM "${UNINST_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKLM "${UNINST_KEY}" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegDWORD HKLM "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKLM "${UNINST_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  WriteRegDWORD HKLM "${UNINST_KEY}" "EstimatedSize" $0
SectionEnd

Section "Right-click: Install with Expunge" SecShell
  ; Adds "Install with Expunge" to the context menu of .exe and .msi files.
  !insertmacro AddShellVerb ".exe"
  !insertmacro AddShellVerb ".msi"
SectionEnd

Section "Desktop shortcut" SecDesktop
  CreateShortcut "$DESKTOP\${APP}.lnk" "$INSTDIR\Expunge.exe"
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\Expunge.exe"
  Delete "$INSTDIR\LICENSE.txt"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${APP}.lnk"
  Delete "$DESKTOP\${APP}.lnk"
  DeleteRegKey HKLM "${UNINST_KEY}"
  DeleteRegKey HKLM "Software\Classes\SystemFileAssociations\.exe\shell\ExpungeInstall"
  DeleteRegKey HKLM "Software\Classes\SystemFileAssociations\.msi\shell\ExpungeInstall"
  ; Install logs and backups in %LOCALAPPDATA%\Expunge are kept on purpose:
  ; they may be needed to undo earlier removals.
SectionEnd
