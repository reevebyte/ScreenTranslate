; ProductName no longer includes the major version. Retire only the known
; previous per-user installation, preserving its application data and keys.
!macro NSIS_HOOK_PREINSTALL
  Push $0
  Push $1
  Push $2
  Push $3
  Push $4
  ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ScreenTranslate 2" "DisplayName"
  ReadRegStr $1 HKCU "Software\github\ScreenTranslate 2" ""
  ${If} $0 == "ScreenTranslate 2"
  ${AndIf} $1 != ""
  ${AndIf} ${FileExists} "$1\uninstall.exe"
    ClearErrors
    ExecWait '"$1\uninstall.exe" /S /UPDATE _?=$1' $2
    ${If} ${Errors}
    ${OrIf} $2 != 0
      MessageBox MB_ICONSTOP "Please close ScreenTranslate 2 and retry the installation."
      Abort
    ${EndIf}
    !insertmacro IsShortcutTarget "$DESKTOP\ScreenTranslate 2.lnk" "$1\screentranslate.exe"
    Pop $2
    ${If} $2 == 1
      Delete "$DESKTOP\ScreenTranslate 2.lnk"
    ${EndIf}
    !insertmacro IsShortcutTarget "$SMPROGRAMS\ScreenTranslate 2.lnk" "$1\screentranslate.exe"
    Pop $2
    ${If} $2 == 1
      Delete "$SMPROGRAMS\ScreenTranslate 2.lnk"
    ${EndIf}
    ReadRegStr $3 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "ScreenTranslate 2"
    ${StrLoc} $4 $3 "$1\screentranslate.exe" ">"
    ${If} $3 != ""
    ${AndIf} $4 != ""
      WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "ScreenTranslate" '$\"$INSTDIR\screentranslate.exe$\" --background'
      DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "ScreenTranslate 2"
    ${EndIf}
  ${EndIf}
  Pop $4
  Pop $3
  Pop $2
  Pop $1
  Pop $0
!macroend
