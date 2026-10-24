; ApricotPlayer 2 Beta (Rust) installer. Built by rust/scripts/build_release.ps1,
; which passes the version, the package folder and the generated registry section.
; It installs per user next to the Python ApricotPlayer, which it never touches.

#ifndef MyAppVersion
#define MyAppVersion "0.0.0"
#endif

#ifndef SourceDir
#define SourceDir "..\rust\local-dist\release\ApricotPlayer2Beta"
#endif

#ifndef OutputDir
#define OutputDir "..\rust\local-dist\release"
#endif

#ifndef RegistryInclude
#define RegistryInclude "..\rust\local-dist\release\media-associations.iss"
#endif

[Setup]
AppId={{5E0B8C77-2A5E-4F53-9C0C-6A2B1D7E4F20}
AppName=ApricotPlayer 2 Beta
AppVersion={#MyAppVersion}
AppVerName=ApricotPlayer 2 Beta {#MyAppVersion}
AppPublisher=ApricotPlayer
AppPublisherURL=https://github.com/Urh2006/ApricotPlayer
AppSupportURL=https://github.com/Urh2006/ApricotPlayer/issues
AppUpdatesURL=https://github.com/Urh2006/ApricotPlayer/releases
DefaultDirName={localappdata}\Programs\ApricotPlayer2Beta
UsePreviousAppDir=yes
DefaultGroupName=ApricotPlayer 2 Beta
DisableProgramGroupPage=yes
OutputDir={#OutputDir}
OutputBaseFilename=ApricotPlayer2BetaSetup
SetupLogging=yes
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayIcon={app}\ApricotPlayer2Beta.exe
UninstallDisplayName=ApricotPlayer 2 Beta
CloseApplications=yes
RestartApplications=no
ChangesAssociations=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional icons:"
Name: "mediaassoc"; Description: "Register ApricotPlayer 2 Beta as a media player for common audio and video files"; GroupDescription: "Windows integration:"

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\ApricotPlayer 2 Beta"; Filename: "{app}\ApricotPlayer2Beta.exe"
Name: "{autodesktop}\ApricotPlayer 2 Beta"; Filename: "{app}\ApricotPlayer2Beta.exe"; Tasks: desktopicon

[Registry]
#include RegistryInclude

[Run]
Filename: "{app}\ApricotPlayer2Beta.exe"; Description: "Launch ApricotPlayer 2 Beta"; Flags: nowait postinstall skipifsilent
