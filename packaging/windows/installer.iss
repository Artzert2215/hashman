; Inno Setup. Build:
;   ISCC /DAppVersion=1.2.3 /DStageDir=<dir with hashman.exe> /DIconFile=<hashman.ico> /DOutDir=<dir> packaging\windows\installer.iss
#ifndef AppVersion
  #error AppVersion must be passed with /DAppVersion=...
#endif
[Setup]
AppId={{8E5C8F49-9A1D-4A2B-9F72-4D8A1E6B5C10}
AppName=Hashman
AppVersion={#AppVersion}
AppPublisher=Artzert
DefaultDirName={autopf}\Hashman
DefaultGroupName=Hashman
OutputDir={#OutDir}
OutputBaseFilename=Hashman-Setup
SetupIconFile={#IconFile}
UninstallDisplayIcon={app}\hashman.exe
ArchitecturesInstallIn64BitMode=x64
PrivilegesRequired=admin
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
[Files]
Source: "{#StageDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs
[Icons]
Name: "{group}\Hashman"; Filename: "{app}\hashman.exe"
Name: "{autodesktop}\Hashman"; Filename: "{app}\hashman.exe"; Tasks: desktopicon
[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional icons:"
