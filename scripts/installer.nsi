; NSIS installer script for PowerBeamer
!include "MUI2.nsh"
!include "FileFunc.nsh"

Name "PowerBeamer"
OutFile "PowerBeamer-windows-x86_64-setup.exe"
InstallDir "$PROGRAMFILES64\PowerBeamer"
InstallDirRegKey HKLM "Software\PowerBeamer" "Install_Dir"
RequestExecutionLevel admin

; UI Settings
!define MUI_ABORTWARNING
!define MUI_ICON "..\ui\icons\app_icon.ico"
!define MUI_UNICON "..\ui\icons\app_icon.ico"

; Pages
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "..\LICENSE"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\powerbeamer.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Launch PowerBeamer"
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

Section "PowerBeamer (required)" SecCore
    SectionIn RO

    SetOutPath "$INSTDIR"
    File /r "..\target\win_pkg\*.*"

    ; Registry keys for uninstaller & program
    WriteRegStr HKLM "Software\PowerBeamer" "Install_Dir" "$INSTDIR"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\PowerBeamer" "DisplayName" "PowerBeamer"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\PowerBeamer" "DisplayIcon" "$INSTDIR\powerbeamer.exe,0"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\PowerBeamer" "Publisher" "Vahid Kalkhorani"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\PowerBeamer" "UninstallString" '"$INSTDIR\uninstall.exe"'
    WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\PowerBeamer" "NoModify" 1
    WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\PowerBeamer" "NoRepair" 1

    WriteUninstaller "$INSTDIR\uninstall.exe"

    ; Start Menu Shortcuts
    CreateDirectory "$SMPROGRAMS\PowerBeamer"
    CreateShortcut "$SMPROGRAMS\PowerBeamer\PowerBeamer.lnk" "$INSTDIR\powerbeamer.exe" "" "$INSTDIR\app_icon.ico"
    CreateShortcut "$SMPROGRAMS\PowerBeamer\Uninstall PowerBeamer.lnk" "$INSTDIR\uninstall.exe"

    ; Desktop Shortcut
    CreateShortcut "$DESKTOP\PowerBeamer.lnk" "$INSTDIR\powerbeamer.exe" "" "$INSTDIR\app_icon.ico"
SectionEnd

Section "Uninstall"
    ; Remove files and directories
    RMDir /r "$INSTDIR"

    ; Remove shortcuts
    RMDir /r "$SMPROGRAMS\PowerBeamer"
    Delete "$DESKTOP\PowerBeamer.lnk"

    ; Remove registry keys
    DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\PowerBeamer"
    DeleteRegKey HKLM "Software\PowerBeamer"
SectionEnd
