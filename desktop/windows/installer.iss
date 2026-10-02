; framecorder's windows installer (inno setup 6), built in ci, see
; .github/workflows/app.yml. it installs for this user only, so it never asks
; for admin: the app goes in %LOCALAPPDATA%\Programs\framecorder, with start
; menu and desktop shortcuts, and it's listed in installed apps to remove.
; the app's own updater runs it with /VERYSILENT, and it starts the app again.
;
; iscc /DVersion=0.1.1 /DExe=<framecorder-desktop.exe> /DOutDir=<dir> installer.iss

#ifndef Version
  #error pass the version, /DVersion=x.y.z
#endif
#ifndef Exe
  #error pass the built app, /DExe=path\to\framecorder-desktop.exe
#endif
#ifndef OutDir
  #define OutDir "."
#endif

[Setup]
; never change this, it's how an update finds the install it replaces
AppId={{71FCF326-64B7-49ED-903C-FC57ED583984}
AppName=framecorder
AppVersion={#Version}
AppVerName=framecorder {#Version}
AppPublisher=coah80
AppPublisherURL=https://framecorder.coah80.com
PrivilegesRequired=lowest
DefaultDirName={localappdata}\Programs\framecorder
DisableDirPage=yes
DisableProgramGroupPage=yes
DisableReadyPage=yes
WizardStyle=modern
SetupIconFile=..\..\app\icons\icon.ico
UninstallDisplayIcon={app}\framecorder.exe
UninstallDisplayName=framecorder
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; an update closes the running app for us, the updater starts it again
CloseApplications=force
RestartApplications=no
OutputDir={#OutDir}
OutputBaseFilename=framecorder-setup
Compression=lzma2
SolidCompression=yes

[Files]
Source: "{#Exe}"; DestDir: "{app}"; DestName: "framecorder.exe"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\framecorder"; Filename: "{app}\framecorder.exe"
Name: "{userdesktop}\framecorder"; Filename: "{app}\framecorder.exe"

[Run]
Filename: "{app}\framecorder.exe"; Description: "open framecorder"; Flags: nowait postinstall skipifsilent
Filename: "{app}\framecorder.exe"; Parameters: "--after-update"; Flags: nowait; Check: WizardSilent

[UninstallRun]
; close this install's app, and only it, before its files go
Filename: "powershell.exe"; Parameters: "-NoProfile -Command ""Get-Process framecorder -ErrorAction SilentlyContinue | Where-Object Path -eq '{app}\framecorder.exe' | Stop-Process -Force"""; Flags: runhidden waituntilterminated; RunOnceId: "CloseApp"

[Registry]
; the app's "start with the computer" setting goes with it. this never writes
; the value, it only removes it on uninstall
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: none; ValueName: "framecorder"; Flags: uninsdeletevalue dontcreatekey
