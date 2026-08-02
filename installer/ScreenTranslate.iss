#define AppName "ScreenTranslate"
#define AppExeName "ScreenTranslate.exe"

#ifndef AppVersion
  #define AppVersion "0.0.0-dev"
#endif
#ifndef AppNumericVersion
  #define AppNumericVersion "0.0.0.0"
#endif
#ifndef SourceExe
  #define SourceExe "..\build\native\Release\ScreenTranslate.exe"
#endif

[Setup]
AppId={{4EF3791F-7F4A-4AEC-89B3-730581B3B571}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=ScreenTranslate
AppPublisherURL=https://github.com/reevebyte/ScreenTranslate
AppSupportURL=https://github.com/reevebyte/ScreenTranslate/issues
AppUpdatesURL=https://github.com/reevebyte/ScreenTranslate/releases
AppMutex=Global\ScreenTranslate.SingleInstance.v1
DefaultDirName={localappdata}\Programs\ScreenTranslate
DefaultGroupName=ScreenTranslate
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir=..\dist
OutputBaseFilename=ScreenTranslate-{#AppVersion}-setup-x64
SetupIconFile=..\app.ico
LicenseFile=..\LICENSE
UninstallDisplayIcon={app}\versions\{#AppVersion}\{#AppExeName}
VersionInfoVersion={#AppNumericVersion}
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
CloseApplicationsFilter={#AppExeName}
RestartApplications=no
ChangesAssociations=no

[Tasks]
Name: "desktopicon"; Description: "创建桌面快捷方式"; GroupDescription: "附加快捷方式"; Flags: unchecked

[Files]
; 每个版本先写入独立目录。复制完成后才清理旧版本，安装失败时仍可回退。
Source: "{#SourceExe}"; DestDir: "{app}\versions\{#AppVersion}"; DestName: "{#AppExeName}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
Source: "..\native\THIRD_PARTY_NOTICES.txt"; DestDir: "{app}"; DestName: "THIRD_PARTY_NOTICES.txt"; Flags: ignoreversion

[Icons]
Name: "{group}\ScreenTranslate"; Filename: "{app}\versions\{#AppVersion}\{#AppExeName}"; WorkingDir: "{app}\versions\{#AppVersion}"
Name: "{autodesktop}\ScreenTranslate"; Filename: "{app}\versions\{#AppVersion}\{#AppExeName}"; WorkingDir: "{app}\versions\{#AppVersion}"; Tasks: desktopicon

[Run]
Filename: "{app}\versions\{#AppVersion}\{#AppExeName}"; Description: "启动 ScreenTranslate"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; 清理升级时由旧安装记录留下的版本目录，不触碰 AppData 中的配置。
Type: filesandordirs; Name: "{app}\versions"

[Code]
procedure RemoveObsoleteProgramFiles;
var
  FindRec: TFindRec;
  VersionsDir, Candidate: String;
begin
  VersionsDir := ExpandConstant('{app}\versions');
  if FindFirst(AddBackslash(VersionsDir) + '*', FindRec) then
  begin
    try
      repeat
        if ((FindRec.Attributes and FILE_ATTRIBUTE_DIRECTORY) <> 0) and
           (FindRec.Name <> '.') and (FindRec.Name <> '..') and
           (CompareText(FindRec.Name, '{#AppVersion}') <> 0) then
        begin
          Candidate := AddBackslash(VersionsDir) + FindRec.Name;
          DelTree(Candidate, True, True, True);
        end;
      until not FindNext(FindRec);
    finally
      FindClose(FindRec);
    end;
  end;

  // 迁移 Python onedir 旧布局时，只清理本程序明确拥有的路径。
  DelTree(ExpandConstant('{app}\_internal'), True, True, True);
  DeleteFile(ExpandConstant('{app}\{#AppExeName}'));
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    RemoveObsoleteProgramFiles;
end;
