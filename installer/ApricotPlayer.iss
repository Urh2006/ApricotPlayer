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
UsePreviousAppDir=no
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
ChangesAssociations=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional icons:"; Flags: unchecked
Name: "mediaassoc"; Description: "Register ApricotPlayer as a media player for common audio and video files"; GroupDescription: "Windows integration:"

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

[Registry]
Root: HKLM; Subkey: "Software\RegisteredApplications"; ValueType: string; ValueName: "ApricotPlayer"; ValueData: "Software\ApricotPlayer\Capabilities"; Tasks: mediaassoc; Flags: uninsdeletevalue
Root: HKLM; Subkey: "Software\ApricotPlayer\Capabilities"; ValueType: string; ValueName: "ApplicationName"; ValueData: "ApricotPlayer"; Tasks: mediaassoc; Flags: uninsdeletekey
Root: HKLM; Subkey: "Software\ApricotPlayer\Capabilities"; ValueType: string; ValueName: "ApplicationDescription"; ValueData: "Accessible media player, YouTube player, downloader, podcast and RSS player"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\ApricotPlayer\Capabilities\FileAssociations"; ValueType: string; ValueName: ".3g2"; ValueData: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\ApricotPlayer\Capabilities\FileAssociations"; ValueType: string; ValueName: ".3gp"; ValueData: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\ApricotPlayer\Capabilities\FileAssociations"; ValueType: string; ValueName: ".aac"; ValueData: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\ApricotPlayer\Capabilities\FileAssociations"; ValueType: string; ValueName: ".aiff"; ValueData: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\ApricotPlayer\Capabilities\FileAssociations"; ValueType: string; ValueName: ".avi"; ValueData: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\ApricotPlayer\Capabilities\FileAssociations"; ValueType: string; ValueName: ".flac"; ValueData: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\ApricotPlayer\Capabilities\FileAssociations"; ValueType: string; ValueName: ".m4a"; ValueData: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\ApricotPlayer\Capabilities\FileAssociations"; ValueType: string; ValueName: ".m4v"; ValueData: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\ApricotPlayer\Capabilities\FileAssociations"; ValueType: string; ValueName: ".mkv"; ValueData: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\ApricotPlayer\Capabilities\FileAssociations"; ValueType: string; ValueName: ".mov"; ValueData: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\ApricotPlayer\Capabilities\FileAssociations"; ValueType: string; ValueName: ".mp3"; ValueData: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\ApricotPlayer\Capabilities\FileAssociations"; ValueType: string; ValueName: ".mp4"; ValueData: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\ApricotPlayer\Capabilities\FileAssociations"; ValueType: string; ValueName: ".mpeg"; ValueData: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\ApricotPlayer\Capabilities\FileAssociations"; ValueType: string; ValueName: ".mpg"; ValueData: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\ApricotPlayer\Capabilities\FileAssociations"; ValueType: string; ValueName: ".oga"; ValueData: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\ApricotPlayer\Capabilities\FileAssociations"; ValueType: string; ValueName: ".ogg"; ValueData: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\ApricotPlayer\Capabilities\FileAssociations"; ValueType: string; ValueName: ".opus"; ValueData: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\ApricotPlayer\Capabilities\FileAssociations"; ValueType: string; ValueName: ".wav"; ValueData: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\ApricotPlayer\Capabilities\FileAssociations"; ValueType: string; ValueName: ".webm"; ValueData: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\ApricotPlayer\Capabilities\FileAssociations"; ValueType: string; ValueName: ".wma"; ValueData: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\ApricotPlayer\Capabilities\FileAssociations"; ValueType: string; ValueName: ".wmv"; ValueData: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\Classes\ApricotPlayer.Media"; ValueType: string; ValueData: "ApricotPlayer media file"; Tasks: mediaassoc; Flags: uninsdeletekey
Root: HKLM; Subkey: "Software\Classes\ApricotPlayer.Media\DefaultIcon"; ValueType: string; ValueData: "{app}\ApricotPlayer.exe,0"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\Classes\ApricotPlayer.Media\shell\open\command"; ValueType: string; ValueData: """{app}\ApricotPlayer.exe"" ""%1"""; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\Classes\SystemFileAssociations\audio\shell\ApricotPlayer"; ValueType: string; ValueName: "MUIVerb"; ValueData: "Play with ApricotPlayer"; Tasks: mediaassoc; Flags: uninsdeletekey
Root: HKLM; Subkey: "Software\Classes\SystemFileAssociations\audio\shell\ApricotPlayer"; ValueType: string; ValueName: "Icon"; ValueData: "{app}\ApricotPlayer.exe,0"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\Classes\SystemFileAssociations\audio\shell\ApricotPlayer\command"; ValueType: string; ValueData: """{app}\ApricotPlayer.exe"" ""%1"""; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\Classes\SystemFileAssociations\video\shell\ApricotPlayer"; ValueType: string; ValueName: "MUIVerb"; ValueData: "Play with ApricotPlayer"; Tasks: mediaassoc; Flags: uninsdeletekey
Root: HKLM; Subkey: "Software\Classes\SystemFileAssociations\video\shell\ApricotPlayer"; ValueType: string; ValueName: "Icon"; ValueData: "{app}\ApricotPlayer.exe,0"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\Classes\SystemFileAssociations\video\shell\ApricotPlayer\command"; ValueType: string; ValueData: """{app}\ApricotPlayer.exe"" ""%1"""; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\Classes\.3g2\OpenWithProgids"; ValueType: none; ValueName: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\Classes\.3gp\OpenWithProgids"; ValueType: none; ValueName: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\Classes\.aac\OpenWithProgids"; ValueType: none; ValueName: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\Classes\.aiff\OpenWithProgids"; ValueType: none; ValueName: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\Classes\.avi\OpenWithProgids"; ValueType: none; ValueName: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\Classes\.flac\OpenWithProgids"; ValueType: none; ValueName: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\Classes\.m4a\OpenWithProgids"; ValueType: none; ValueName: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\Classes\.m4v\OpenWithProgids"; ValueType: none; ValueName: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\Classes\.mkv\OpenWithProgids"; ValueType: none; ValueName: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\Classes\.mov\OpenWithProgids"; ValueType: none; ValueName: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\Classes\.mp3\OpenWithProgids"; ValueType: none; ValueName: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\Classes\.mp4\OpenWithProgids"; ValueType: none; ValueName: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\Classes\.mpeg\OpenWithProgids"; ValueType: none; ValueName: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\Classes\.mpg\OpenWithProgids"; ValueType: none; ValueName: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\Classes\.oga\OpenWithProgids"; ValueType: none; ValueName: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\Classes\.ogg\OpenWithProgids"; ValueType: none; ValueName: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\Classes\.opus\OpenWithProgids"; ValueType: none; ValueName: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\Classes\.wav\OpenWithProgids"; ValueType: none; ValueName: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\Classes\.webm\OpenWithProgids"; ValueType: none; ValueName: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\Classes\.wma\OpenWithProgids"; ValueType: none; ValueName: "ApricotPlayer.Media"; Tasks: mediaassoc
Root: HKLM; Subkey: "Software\Classes\.wmv\OpenWithProgids"; ValueType: none; ValueName: "ApricotPlayer.Media"; Tasks: mediaassoc

[Run]
Filename: "{app}\ApricotPlayer.exe"; Description: "Launch ApricotPlayer"; Flags: nowait postinstall skipifsilent
