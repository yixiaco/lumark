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
; 安装器始终请求管理员权限；admin 模式下 {autopf} 解析为 Program Files，
; {autoprograms}/{autodesktop} 自动使用公共开始菜单和公共桌面。
PrivilegesRequired=admin
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
; 用启动安装器的原始用户身份注册当前用户右键菜单，避免写入提权管理员的 HKCU。
; postinstall 默认就是 runasoriginaluser，这里显式写出便于审阅。
Filename: "{app}\Lumark.exe"; Parameters: "--register-shell"; Flags: runhidden nowait runasoriginaluser
Filename: "{app}\Lumark.exe"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent runasoriginaluser

[UninstallRun]
; Inno 的 runasoriginaluser 只允许用于 [Run]；卸载器按当前提权用户执行，
; 同一用户卸载时 HKCU 一致，管理员代其他用户卸载会保留该用户的菜单项。
Filename: "{app}\Lumark.exe"; Parameters: "--unregister-shell"; Flags: runhidden; RunOnceId: "UnregisterShell"
