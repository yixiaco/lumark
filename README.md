# Lumark

Lumark 是一个使用 Rust + [gpui-kit](https://github.com/longbridge/gpui-kit) 构建的 Windows 优先桌面压缩管理器。

## 功能介绍

- 压缩后端使用 zlib-ng 和新版 ZIP/7Z 编码器，ZIP 按 1 MiB 分块读写并按块检查取消；7Z 多线程 LZMA2 默认按 16 MiB 编码块完成量报告真实进度（字典更大时随库钳制），两次块事件之间按最近速度平滑显示并在块完成时校正；单线程/其他算法按已写入编码器的字节更新，加密归档保留读取进度，7Z 解压按 256 KiB 分块、100 毫秒节流上报单文件内部进度。
- ZIP、7Z、LZH、TAR、TAR.GZ/BZ2/XZ/ZST/BR/LZ4/LZ/LZMA；GZ、BZ2、XZ、ZSTD、Brotli、LZ4、Snappy、BZip3；ZIP/7Z AES 密码、可选的 ZipCrypto 兼容写入和 7Z 固实压缩；ZIPX/旧 ZIP 的 Deflate64、Shrink、Reduce、Implode 只读解压。LZH 创建底层支持 lh0、lh4–lh7，界面预设使用 lh4/lh5/lh7；lh0 条目已分块流式写入，lh4/lh5/lh7 仍需要整块输入，超过 512 MiB 会提前拒绝。
- ZPAQ 读写（内嵌 libzpaq/JIDAC 引擎）：创建标准归档、AES-256 密码、列表、完整测试、预算解压、分卷和密码库联动；压缩级别 1–5 可选，轻巧模式的“体积小”对应 5 级，默认保留源文件原始时间，≥32 MiB 的单文件批次交给可杀死的隐藏工作进程执行，压缩中可直接取消；解压和完整测试也通过该工作进程执行，任务取消时终止子进程并清理暂存区；普通小文件批次按 32 MiB / 64 文件分批上报进度。创建时会跳过符号链接和 Windows 目录联接，避免归档条目与源计划不一致；输出同名旧压缩包在任务成功后原子替换。PAQ8/LPAQ 系列暂无许可兼容的成熟后端，不在支持范围。
- RAR 只读列表、密码验证、解压和完整性测试（UnRAR 后端，保留许可声明）；Unix `.Z`、AES `.enc`、ALZ（Store/Deflate/ALZ 改造 Bzip2，支持 ZipCrypto 和 `.a00` 分卷）只读解压。
- ISO/UDF、CAB、ARJ、ACE、ARC/PAK、Quake/Chromium/Unreal PAK、CPIO、WIM（第一映像，XPRESS/LZX/LZMS，支持 SWM 分卷）、DEB、XAR/PKG、RPM、MSI、CHM、SquashFS、NSIS、DMG、EGG、ISZ、IMG/BIN、UU/XXE、LZ/LZMA、TAR.Z、ZOO、HA、HYP、SQ、SQZ、UC2、ICE、PACKICE 等 40+ 只读格式适配；WIM 支持 LZX 压缩创建。
- ZIP/7Z 归档内浏览、选择性解压、删除/重命名/添加文件，7Z 重命名和整块删除直接复用原始压缩块，无法复用时才全量重建，最后原子替换；把文件或文件夹拖入归档时可选择“替换同名文件”“只更新较新的文件”“仅刷新已有条目”或“同步文件夹内容”，同步会删除归档中磁盘已不存在的条目，结果消息会给出跳过和删除数量；可选 `.lumark.bak`。
- `.001` 原始分卷创建、列表和解压；失败时保留旧卷并清理半成品。
- 压缩源为多个文件或文件夹时自动改用支持多文件的格式：GZ/BZ2/XZ/ZST/Brotli/LZ4/Snappy/BZip3 等单文件流格式有 TAR 变体时切换为对应 TAR，Snappy/BZip3 回退 ZIP。
- 密码管理：添加/编辑/删除即时写入、名称/备注/匹配规则、记录时间、掩码与 10 秒显示、Windows DPAPI（计算机绑定）、可选主密码、Windows 会话锁定时自动锁定、加密导入导出和明文 CSV 导出；导出位置通过文件夹选择器指定，锁定时需先解锁，操作结果通过通知提示；密码库位置可查看并迁移到自定义文件夹。
- 打开加密压缩包时提示输入密码，支持显示密码、自动匹配密码库记录和跳转密码管理；自动匹配失败后可直接勾选“保存到密码库”，密码校验通过才写入记录。
- 支持把归档内文件直接拖到资源管理器解压，或用 Ctrl+C 复制文件后粘贴；临时文件按归档窗口隔离，关闭窗口后清理。
- 解压加密 ZIP/7Z/RAR 时从密码库生成候选密码并在后台逐个验证；主密码锁定时提供解锁入口，校验较慢时可停止自动尝试并改为手动输入。
- 后台任务：固定工作线程池按提交顺序排队执行归档加载、压缩、解压、测试、编辑和记录密码验证，排队中的任务可直接取消；支持进度、暂停/继续、取消，任务中心同时显示运行中/排队中/已完成数量和跨任务合并进度。
- 压缩后可测试归档并在成功时删除源文件。
- 压缩完成邮件通知：可配置 SMTP 主机、端口、1–300 秒超时、账号、DPAPI 加密密码/XOAUTH2 访问令牌/客户端密钥/刷新令牌、发件人和收件人，支持隐式 TLS/STARTTLS/无 TLS 及测试发送；瞬时网络/4xx 错误最多退避重试 3 次，保留最近 8 条发送结果，并把失败通知持久化到待重试队列；配置刷新令牌后通过 `reqwest` 从令牌端点自动获取访问令牌。
- 自解压 EXE：压缩时可勾选“自解压 EXE”（ZIP 或 7Z 单卷载荷，可带密码），生成的可执行文件内置 `lumark-sfx` 运行时；双击后可选择目标目录（支持浏览）、按需输入密码并执行解压，解压过程显示进度并可取消；也支持 `/S`、`/D=`、`/P=`、`/Y` 静默参数，静默模式不初始化界面且失败返回非零退出码；密码也可通过 `LUMARK_SFX_PASSWORD` 环境变量提供，避免出现在命令行历史中；可选择 `.ico` 自定义图标，成功后打开资源管理器，模块随安装脚本分发；运行时界面按 Windows 显示语言切换中英文，也可用 `LUMARK_LANG=en` 或命令行 `/LANG=en` 覆盖。
- 伪装压缩（全格式）：压缩时可勾选“伪装为媒体文件”，把 ZIP、7Z、LZH、ZPAQ、TAR 系列、GZ/BZ2/XZ/ZST/Brotli/LZ4 的输出包装成可正常播放/查看的 MP4/JPG 外观；ZIP、7Z、LZH、TAR 系列和单文件流边生成边写入伪装容器，不再额外保留完整内层文件；内层归档格式保持不变，载荷按 128 KiB 分块用 BLAKE3 密钥流混淆，尾部 512 字节记录载荷偏移/长度、原文件名、混淆盐和载荷类型。内置视频/图片面具，也可选择自定义媒体文件；伪装不提供加密。
- 一键打开与一键还原：双击伪装归档、或资源管理器“用 Lumark 打开”，直接浏览/选择性解压/测试内层归档；ZIP/7Z/TAR 系列/单文件流/LZH 使用读取视图不复制载荷，ZPAQ/RAR/ALZ/传统只读格式物化到临时目录并做进程内缓存。右键“还原伪装文件”或资源管理器“Lumark 一键还原”把真身写回伪装文件旁，同名文件自动追加序号；伪装 ZIP/7Z 还支持原地重命名、删除和添加，自动复用原面具重新包装。
- 任意文件伪装：文件或归档右键“伪装为媒体文件…”打开独立伪装窗口，资源管理器右键菜单也提供“伪装为媒体文件…”入口；可选择输出目录、面具和“伪装后删除源文件”；已有 ZIP/7Z 等归档伪装后仍可直接浏览，普通文件伪装只能还原。
- 可配置新压缩默认启用伪装、默认面具类型和自定义面具文件；`.mp4`/`.jpg` 只注册 `OpenWithProgids` 作为“打开方式”候选，不修改系统默认关联。
- 解压资源预算：默认单次解压总量 128 GiB、条目 100 万，超出时中止并删除半成品；ZIP/7Z/TAR 在写文件前检查，单文件流、加密容器和容器复制按剩余预算分块中止；RAR、ALZ 和传统只读格式按条目声明大小做预算检查，ALZ 的写入器还会按剩余预算兜底。
- 压缩后关机：压缩时可勾选“完成后关闭计算机”，压缩、测试和删除源文件都成功后由系统 `shutdown.exe` 安排 60 秒倒计时关机，期间可用 `shutdown /a` 取消；失败或取消的任务不会关机。
- CPU 占用：压缩时可勾选“降低压缩优先级”，任务运行期间进程以普通以下优先级调度，全部低优先级任务结束后恢复；压缩线程数仍可单独限制。
- Windows 当前用户文件关联：可勾选 ZIP、ZIPX、7Z、RAR、XAR/PKG、RPM、MSI、CHM、SquashFS、DMG、EGG、Snappy、BZip3、ASAR、PAK、APK、JAR、PAX 等 42 种压缩格式，注册后直接打开 Lumark 的 Windows 默认应用页，由系统确认默认值；同时提供经典右键菜单、Send To、回收站删除和安装/卸载脚本。
- ISO 虚拟光驱：Lumark 文件列表和资源管理器右键菜单都提供“加载至虚拟光驱/卸载虚拟光驱”；挂载成功后自动打开盘符，使用 Windows 8+ 自带的 `Mount-DiskImage`/`Dismount-DiskImage`。客户端普通用户通常无需管理员，Server 或收紧的安全策略可能要求管理员。
- 单实例多窗口：首个进程创建命名管道和互斥体，后续进程优先通过命名管道转发参数（失败回退请求目录）；每个文件生成独立窗口，窗口之间共享设置和密码库、各自保留浏览状态，关闭全部窗口后退出进程。
- 内置查看器：文本/十六进制按内容自动选择，支持编辑器保存和归档条目写回；exe/dll 等 PE 文件默认作为只读归档浏览，按节和 `.rsrc` 资源目录展开，图标资源重建为 `.ico`，可解压节/资源并用内置查看器或图片查看器打开，右键“运行”才交给系统执行；图片查看器支持 PNG/JPEG/GIF/WebP/SVG/BMP/TIFF/ICO 等常见格式，支持适应窗口、100%、缩放和滚动平移。归档条目先解压到临时目录再打开，临时文件沿用现有查看器生命周期。
- 游戏资产：Windows 上提供游戏库、游戏目录扫描、AES 密钥提取/手动校验、Oodle 后端与 `.usmap` 配置；可同时打开多个游戏并各自保留独立浏览位置和历史，打开列表在重启后自动恢复；IoStore/PAK 只读浏览复用现有文件表，支持 UTexture2D（BC1/BC3/BC4/BC5/BC7/BGRA8/RGBA8/G8）、StaticMesh/SkeletalMesh 多 LOD 与查看器切换（含逐 LOD 蒙皮权重、LBS 蒙皮变换、动画播放/暂停、section/材质切换）、Skeleton 参考骨骼、AnimSequence 元数据/PerTrack 解码与时间轴、Zen/FF7R 只读字段行。查看器支持左键旋转、右键平移、滚轮缩放；详情缩略图与查看器共用同一 Snooper 光照/背景预设；检查器可查看/切换 section 可见性、材质 blend/shading、高光强度、粗糙度区间、AO/自发光与贴图尺寸，默认隐藏无贴图/半透明 section；可打开导出表 + 属性树的只读 JSON 视图。
- 密码库 XChaCha20-Poly1305、Argon2id、原子写入、损坏 `.bak` 恢复、外部修改锁定和密码零化。

### 格式支持矩阵

- **读写**：ZIP、ZIPX（AES/传统加密、Deflate/Bzip2/XZ/Zstd/PPMd 写入；LZMA/Deflate64 只读）、7Z、LZH（lh0/lh4–lh7）、ZPAQ、WIM（LZX 创建）、TAR、PAX、APK、JAR、ASAR（无密码）、TAR.GZ、TAR.BZ2、TAR.XZ、TAR.ZST、TAR.BR、TAR.LZ4、TAR.LZ、TAR.LZMA、GZ、BZ2、XZ、ZST、Brotli、LZ4、Snappy Framed（`.sz`/`.snappy`）、BZip3（`.bz3`）。
- **只读解压**：PE（节/资源目录，图标组重建为 ICO）、RAR、ISO/UDF、CAB、ARJ、ACE、ARC/PAK、Quake PAK、Chromium PAK、Unreal PAK（版本 1–11，AES-256，Oodle 优先内置 Rust 解码、可选 DLL）、CPIO、WIM（仅第一映像，XPRESS/LZX/LZMS，支持 SWM）、DEB、ISZ、IMG/BIN、UU/XXE、LZ/LZMA、ALZ、TAR.Z、ZOO、HA、HYP、SQ/SQ2/QQQ、SQZ、UC2、ICE、PACKICE；ZIPX/旧 ZIP 的 Deflate64、Shrink、Reduce、Implode。
- **伪装**：所有可写格式都能输出为媒体外观并直接浏览/解压/测试/还原；只读归档可通过“伪装为媒体文件…”包装，打开时物化到临时目录后复用现有读取逻辑；原始文件载荷只能还原，不能浏览。
- **格式支持范围**：PAQ8/LPAQ 按许可与兼容性评估不纳入；MSI 提供内嵌及同目录外部 CAB 读取与目录层级还原；WIM 的 LZMS/SWM 写入、RAR/CAB/ISO/UDF/ALZ 创建、ZIP Deflate64 等旧方法与 `.tar.Z` 写入不在当前格式矩阵内。

## 命令

### 启动

```powershell
cargo run -p lumark-app
```

### 构建

```powershell
cargo build --release --locked -p lumark-app
cargo build --release --locked -p lumark-sfx
```

### 发布构建与签名

```powershell
./tools/build_release.ps1 -Version 0.1.0 -Arch x64
# 首次构建 x86 前安装目标
rustup target add i686-pc-windows-msvc
./tools/build_release.ps1 -Version 0.1.0 -Arch x86
./tools/build_release.ps1 -Version 0.1.0 -Arch x64 -UseDevCert
./tools/build_release.ps1 -Version 0.1.0 -OodleDll .\oo2core_9_win64.dll
./installer/pack-installer.ps1 -Version 0.1.0 -Arch x64
./installer/pack-installer.ps1 -Version 0.1.0 -Arch x86
./installer/pack-installer.ps1 -Version 0.1.0 -Arch x64 -SignThumbprint <SHA1指纹>
./installer/pack-installer.ps1 -Version 0.1.0 -Arch x86 -SignPfxPath .\codesign.pfx -SignPfxPassword <密码>
./installer/self-sign.ps1
./installer/sign.ps1 -VerifyOnly
```

### 测试

```powershell
cargo test --workspace
powershell -NoProfile -ExecutionPolicy Bypass -File tools/test_workspace.ps1
powershell -NoProfile -File tools/check_file_sizes.ps1
```

### 基准与模糊测试

```powershell
cargo +nightly fuzz run archive_list
cargo run -p lumark-browser --release --example bench_100k
cargo run -p lumark-archive --release --example bench_archive
```

### 发布

```powershell
git tag v0.1.0
git push origin v0.1.0
```