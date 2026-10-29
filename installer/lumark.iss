; Lumark Windows 安装程序（Inno Setup 6）。
;
; 先由 installer/pack-installer.ps1 把 release 产物暂存到 dist\windows，
; 再用 ISCC 编译本脚本。官方文档建议 Windows 用户安装器使用 Inno Setup；
; 稀疏 MSIX 只是可选的 Windows 11 现代右键菜单集成，不作为主分发格式。

#define AppName "Lumark"
#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif

[Setup]
AppId={{E7E2B7E2-5FC4-4A40-9F72-2EFFA3E5B9A1}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=Lumark
DefaultDirName={autopf}\Lumark
DefaultGroupName=Lumark
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=..\dist
OutputBaseFilename=Lumark-Setup-{#AppVersion}
SetupIconFile=..\crates\lumark-app\assets\lumark.ico
UninstallDisplayIcon={app}\Lumark.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; 使用 64 位 Setup Loader，减少 32 位安全软件注入导致的临时目录创建失败。
UseSetupLdr=x64

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\dist\windows\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\Lumark"; Filename: "{app}\Lumark.exe"
Name: "{autodesktop}\Lumark"; Filename: "{app}\Lumark.exe"; Tasks: desktopicon

[Run]
; 安装后注册当前用户的经典右键菜单。
Filename: "{app}\Lumark.exe"; Parameters: "--register-shell"; Flags: runhidden nowait
Filename: "{app}\Lumark.exe"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{app}\Lumark.exe"; Parameters: "--unregister-shell"; Flags: runhidden; RunOnceId: "UnregisterShell"
