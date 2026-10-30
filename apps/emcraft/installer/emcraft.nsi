; EMCraft's Windows installer (NSIS 3, Modern UI 2). Built by `cargo xtask installer-windows`
; (docs/install-windows.md), which stages the files and defines:
;   VERSION       the app's version (Cargo.toml)
;   FILE_VERSION  the same as four numbers (0.1.0.0)
;   STAGE         the folder with emcraft.exe, emcraft-cli.exe, emcraft.ico and LICENSE.txt
;   OUTFILE       the installer to write
;
; Per-user: no administrator rights, installed under %LOCALAPPDATA%\Programs\EMCraft, every
; registry entry under HKCU (Start menu shortcuts, the .emcraft file type, Apps & features).
; The CLI is not added to PATH (see docs/install-windows.md): the "EMCraft Command Prompt"
; shortcut opens a terminal where `emcraft-cli` works.

!macro Require NAME
  !ifndef ${NAME}
    !error "${NAME} is not defined: build with cargo xtask installer-windows"
  !endif
!macroend
!insertmacro Require VERSION
!insertmacro Require FILE_VERSION
!insertmacro Require STAGE
!insertmacro Require OUTFILE

Unicode true
ManifestDPIAware true
SetCompressor /SOLID lzma
RequestExecutionLevel user

!define APP "EMCraft"
!define PROGID "EMCraft.Project"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\EMCraft"
!define APP_PATHS "Software\Microsoft\Windows\CurrentVersion\App Paths"

Name "${APP}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\EMCraft"
InstallDirRegKey HKCU "Software\EMCraft" "InstallDir"
BrandingText "${APP} ${VERSION}"

VIProductVersion "${FILE_VERSION}"
VIFileVersion "${FILE_VERSION}"
VIAddVersionKey "ProductName" "${APP}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "${APP} ${VERSION} setup"
VIAddVersionKey "CompanyName" "EMCraft contributors"
VIAddVersionKey "LegalCopyright" "EMCraft contributors, MIT OR Apache-2.0"

!include "MUI2.nsh"

!define MUI_ICON "${STAGE}\emcraft.ico"
!define MUI_UNICON "${STAGE}\emcraft.ico"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\emcraft.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Start EMCraft"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "${STAGE}\LICENSE.txt"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

; Tells Explorer that file types changed (SHCNE_ASSOCCHANGED).
!macro NotifyAssociations
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend

Section "EMCraft" SecMain
  SectionIn RO
  SetOutPath "$INSTDIR"
  File "${STAGE}\emcraft.exe"
  File "${STAGE}\emcraft-cli.exe"
  File "${STAGE}\emcraft.ico"
  File "${STAGE}\LICENSE.txt"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  WriteRegStr HKCU "Software\EMCraft" "InstallDir" "$INSTDIR"

  ; Start menu
  CreateShortCut "$SMPROGRAMS\EMCraft.lnk" "$INSTDIR\emcraft.exe" "" "$INSTDIR\emcraft.exe" 0
  CreateShortCut "$SMPROGRAMS\EMCraft Command Prompt.lnk" "$SYSDIR\cmd.exe" \
    '/k "set "PATH=$INSTDIR;%PATH%" && emcraft-cli --help"' "$INSTDIR\emcraft.ico" 0

  ; `emcraft` and `emcraft-cli` from the Run box and `start`
  WriteRegStr HKCU "${APP_PATHS}\emcraft.exe" "" "$INSTDIR\emcraft.exe"
  WriteRegStr HKCU "${APP_PATHS}\emcraft-cli.exe" "" "$INSTDIR\emcraft-cli.exe"

  ; .emcraft projects open in the app: emcraft.exe "<file>"
  WriteRegStr HKCU "Software\Classes\.emcraft" "" "${PROGID}"
  WriteRegStr HKCU "Software\Classes\.emcraft\OpenWithProgids" "${PROGID}" ""
  WriteRegStr HKCU "Software\Classes\${PROGID}" "" "EMCraft project"
  WriteRegStr HKCU "Software\Classes\${PROGID}\DefaultIcon" "" "$INSTDIR\emcraft.exe,0"
  WriteRegStr HKCU "Software\Classes\${PROGID}\shell\open\command" "" '"$INSTDIR\emcraft.exe" "%1"'
  !insertmacro NotifyAssociations

  ; Apps & features
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${APP}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "EMCraft contributors"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\emcraft.exe,0"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
  SectionGetSize ${SecMain} $0
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "EstimatedSize" $0
SectionEnd

; Settings in %APPDATA%\emcraft and the user's projects are kept.
Section "Uninstall"
  Delete "$INSTDIR\emcraft.exe"
  Delete "$INSTDIR\emcraft-cli.exe"
  Delete "$INSTDIR\emcraft.ico"
  Delete "$INSTDIR\LICENSE.txt"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\EMCraft.lnk"
  Delete "$SMPROGRAMS\EMCraft Command Prompt.lnk"

  DeleteRegKey HKCU "${APP_PATHS}\emcraft.exe"
  DeleteRegKey HKCU "${APP_PATHS}\emcraft-cli.exe"

  DeleteRegKey HKCU "Software\Classes\${PROGID}"
  DeleteRegValue HKCU "Software\Classes\.emcraft\OpenWithProgids" "${PROGID}"
  DeleteRegKey /ifempty HKCU "Software\Classes\.emcraft\OpenWithProgids"
  ReadRegStr $0 HKCU "Software\Classes\.emcraft" ""
  StrCmp $0 "${PROGID}" 0 +2
    DeleteRegValue HKCU "Software\Classes\.emcraft" ""
  DeleteRegKey /ifempty HKCU "Software\Classes\.emcraft"
  !insertmacro NotifyAssociations

  DeleteRegKey HKCU "${UNINSTALL_KEY}"
  DeleteRegKey HKCU "Software\EMCraft"
SectionEnd
