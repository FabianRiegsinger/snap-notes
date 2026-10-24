; Inno Setup script for the Windows installer.
; Build: iscc /DVersion=<x.y.z> /O<out dir> /F<name> packaging\windows\snap-notes.iss
; Installs per user (no admin prompt), adds Start menu entries and an uninstaller.

#ifndef Version
  #define Version "0.0.0"
#endif

[Setup]
AppId={{6E0D5C2A-3B7F-4C61-9D2E-5A8B1F4C7E90}
AppName=Snap Notes
AppVersion={#Version}
AppPublisher=Fabian Riegsinger
DefaultDirName={autopf}\Snap Notes
DefaultGroupName=Snap Notes
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
SetupIconFile=..\..\assets\icon.ico
UninstallDisplayIcon={app}\snap-notes.exe
LicenseFile=..\..\LICENSE
Compression=lzma2
SolidCompression=yes
WizardStyle=modern

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\..\target\release\snap-notes.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Snap Notes"; Filename: "{app}\snap-notes.exe"
Name: "{group}\Uninstall Snap Notes"; Filename: "{uninstallexe}"
Name: "{autodesktop}\Snap Notes"; Filename: "{app}\snap-notes.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\snap-notes.exe"; Description: "{cm:LaunchProgram,Snap Notes}"; Flags: nowait postinstall skipifsilent
