; Lumark Windows 安装程序（Inno Setup 6）。
;
; 同一份脚本按架构编译：installer/pack-installer.ps1 通过 ISCC 传入
; /DAppVersion、/DArch、/DStageDir 和 /DOutputDir，并加 /DX64 或 /DX86，
; 把目标架构的 release 产物暂存到 target\dist-stage\<架构> 后再编译。
; 官方文档建议 Windows 用户安装器使用 Inno Setup；稀疏 MSIX 只是可选的
; Windows 11 现代右键菜单集成，不作为主分发格式。

#define AppName "Lumark"
#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif
#ifndef Arch
  #define Arch "x64"
#endif
#ifndef StageDir
  #define StageDir "..\target\dist-stage\x64"
#endif
#ifndef OutputDir
  #define OutputDir "..\dist"
#endif

[Setup]
#ifdef X64
; x64 保留历史 AppId，已发布的 0.1.x x64 安装可继续按升级流程覆盖。
AppId={{E7E2B7E2-5FC4-4A40-9F72-2EFFA3E5B9A1}
UninstallDisplayName=Lumark
#else
; Inno 把 32/64 位安装模式视为不同应用（side-by-side），x86 必须使用独立
; AppId，否则两个架构会各装一份并把开始菜单、右键菜单注册互相覆盖。
AppId={{3F9C1E4A-6B2D-4C7E-9A51-2D8B7F0C4E63}
UninstallDisplayName=Lumark (x86)
#endif
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=Lumark
DefaultDirName={autopf}\Lumark
DefaultGroupName=Lumark
DisableProgramGroupPage=yes
; 安装器始终请求管理员权限；admin 模式下 {autopf} 解析为 Program Files，
; x86 安装包在 64 位系统上解析为 Program Files (x86)，在 32 位系统上仍是 Program Files。
PrivilegesRequired=admin
OutputDir={#OutputDir}
OutputBaseFilename=Lumark-Setup-{#AppVersion}-{#Arch}
SetupIconFile=..\crates\lumark-app\assets\lumark.ico
UninstallDisplayIcon={app}\Lumark.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
#ifdef X64
; x64 安装包只允许跑在 64 位 Windows；按现有约定使用 64 位 Setup Loader，
; 减少 32 位安全软件注入导致的临时目录创建失败。
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UseSetupLdr=x64
#else
; x86 安装包必须能在 32 位和 64 位 Windows 上运行：允许 x86compatible，
; 保持 32 位安装模式（不设置 ArchitecturesInstallIn64BitMode），并使用
; 32 位 Setup Loader，否则纯 32 位系统无法启动安装器。
ArchitecturesAllowed=x86compatible
UseSetupLdr=x86
#endif

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#StageDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

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

[Code]
// Inno 按 32/64 位安装模式区分应用，两个架构允许并存会让开始菜单快捷方式
// 和当前用户右键菜单互相覆盖；这里检测另一架构的卸载键并阻止双装。
// x64 安装包检查 32 位注册表视图；x86 安装包在 64 位系统上检查 64 位视图。
function OtherArchitectureInstalled: Boolean;
var
  UninstallCommand: String;
begin
#ifdef X64
  Result := RegQueryStringValue(
    HKLM32,
    'SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{3F9C1E4A-6B2D-4C7E-9A51-2D8B7F0C4E63}_is1',
    'UninstallString', UninstallCommand);
#else
  Result := False;
  if IsWin64 then
    Result := RegQueryStringValue(
      HKLM64,
      'SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{E7E2B7E2-5FC4-4A40-9F72-2EFFA3E5B9A1}_is1',
      'UninstallString', UninstallCommand);
#endif
end;

function InitializeSetup: Boolean;
begin
  Result := not OtherArchitectureInstalled;
  if not Result then
    MsgBox('Lumark is already installed with the other architecture.' + #13#10 +
           'Please uninstall it from Apps & features, then run this installer again.',
      mbError, MB_OK);
end;
