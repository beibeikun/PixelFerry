Unicode true
!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "x64.nsh"
Name "PixelFerry"
OutFile "${PROJECT_ROOT}\release\PixelFerry-Setup-0.2.0.exe"
InstallDir "$LOCALAPPDATA\Programs\PixelFerry"
RequestExecutionLevel user
SetCompressor /SOLID lzma
!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "SimpChinese"
!insertmacro MUI_LANGUAGE "English"
Function .onInit
  ${IfNot} ${RunningX64}
    MessageBox MB_ICONSTOP "PixelFerry requires Windows x64."
    Abort
  ${EndIf}
FunctionEnd
Section "PixelFerry" Main
  SetRegView 32
  ReadRegStr $0 HKLM "SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" "pv"
  ${If} $0 == ""
  ${OrIf} $0 == "0.0.0.0"
    ReadRegStr $0 HKCU "SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" "pv"
  ${EndIf}
  ${If} $0 == ""
  ${OrIf} $0 == "0.0.0.0"
    InitPluginsDir
    SetOutPath "$PLUGINSDIR"
    File "${PROJECT_ROOT}\resources\MicrosoftEdgeWebview2Setup.exe"
    DetailPrint "Installing Microsoft Edge WebView2 Runtime (internet required)..."
    ExecWait '"$PLUGINSDIR\MicrosoftEdgeWebview2Setup.exe" /silent /install' $1
    ${If} $1 != 0
      MessageBox MB_ICONSTOP "WebView2 Runtime installation failed. Check your internet connection, install Microsoft Edge WebView2 Runtime, and run setup again."
      Abort
    ${EndIf}
  ${EndIf}
  SetOutPath "$INSTDIR"
  File /r "${PROJECT_ROOT}\release\PixelFerry\*"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateDirectory "$SMPROGRAMS\PixelFerry"
  CreateShortcut "$SMPROGRAMS\PixelFerry\PixelFerry.lnk" "$INSTDIR\pixelferry.exe"
  CreateShortcut "$SMPROGRAMS\PixelFerry\Uninstall.lnk" "$INSTDIR\Uninstall.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\PixelFerry" "DisplayName" "PixelFerry"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\PixelFerry" "DisplayVersion" "0.2.0"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\PixelFerry" "UninstallString" '$\"$INSTDIR\Uninstall.exe$\"'
SectionEnd
Section "Uninstall"
  Delete "$SMPROGRAMS\PixelFerry\PixelFerry.lnk"
  Delete "$SMPROGRAMS\PixelFerry\Uninstall.lnk"
  RMDir "$SMPROGRAMS\PixelFerry"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\PixelFerry"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "PixelFerry"
  Delete "$INSTDIR\pixelferry.exe"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\THIRD_PARTY.md"
  Delete "$INSTDIR\Skia-LICENSE.txt"
  Delete "$INSTDIR\Cargo.lock"
  Delete "$INSTDIR\package-lock.json"
  Delete "$INSTDIR\MicrosoftEdgeWebview2Setup.exe"
  Delete "$INSTDIR\vcruntime140.dll"
  Delete "$INSTDIR\vcruntime140_1.dll"
  Delete "$INSTDIR\msvcp140.dll"
  Delete "$INSTDIR\msvcp140_1.dll"
  Delete "$INSTDIR\msvcp140_2.dll"
  Delete "$INSTDIR\msvcp140_atomic_wait.dll"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir /r "$INSTDIR\tools"
  RMDir /r "$INSTDIR\licenses"
  RMDir /r "$INSTDIR\docs"
  RMDir "$INSTDIR"
SectionEnd
