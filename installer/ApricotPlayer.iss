#ifndef MyAppVersion
#define MyAppVersion "0.0.0"
#endif

#ifndef SourceExe
#define SourceExe "..\release-dist\ApricotPlayer.exe"
#endif

#ifndef SourceDir
#define SourceDir ""
#endif

#ifndef OutputDir
#define OutputDir "..\release-dist"
#endif

[Setup]
AppId={{8A11B502-0463-48B7-B43B-C9D27A7D7F9F}
AppName=ApricotPlayer
AppVersion={#MyAppVersion}
AppPublisher=ApricotPlayer
AppPublisherURL=https://github.com/Urh2006/ApricotPlayer
AppSupportURL=https://github.com/Urh2006/ApricotPlayer/issues
AppUpdatesURL=https://github.com/Urh2006/ApricotPlayer/releases/latest
DefaultDirName={autopf}\ApricotPlayer
DefaultGroupName=ApricotPlayer
DisableProgramGroupPage=yes
OutputDir={#OutputDir}
OutputBaseFilename=ApricotPlayerSetup
SetupLogging=yes
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayIcon={app}\ApricotPlayer.exe
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional icons:"; Flags: unchecked

[Files]
#if SourceDir != ""
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
#else
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "ApricotPlayer.exe"; Flags: ignoreversion
#endif

[InstallDelete]
Type: filesandordirs; Name: "{app}\_internal"
Type: files; Name: "{userdesktop}\ApricotPlayer.lnk"
Type: files; Name: "{userdesktop}\apricotplayer.lnk"
Type: files; Name: "{autodesktop}\ApricotPlayer.lnk"
Type: files; Name: "{autodesktop}\apricotplayer.lnk"

[Icons]
Name: "{group}\ApricotPlayer"; Filename: "{app}\ApricotPlayer.exe"
Name: "{autodesktop}\ApricotPlayer"; Filename: "{app}\ApricotPlayer.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\ApricotPlayer.exe"; Description: "Launch ApricotPlayer"; Flags: nowait postinstall skipifsilent
