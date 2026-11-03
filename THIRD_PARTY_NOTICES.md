# Third-Party Notices

Lumark is licensed under MIT OR Apache-2.0. The repository uses or references the following projects:

- **gpui-kit** — Apache-2.0 — https://github.com/longbridge/gpui-kit
- **GPUI / gpui-pre** — Apache-2.0 — https://github.com/zed-industries/zed
- **zip** — MIT — https://github.com/zip-rs/zip2
- **sevenz-rust2** — Apache-2.0 — https://github.com/hasenbanck/sevenz-rust2
- **GeeZipX** — MIT — https://github.com/GEELINX-LTD/geezipx — `crates/lumark-legacy` 的裁剪来源
- **lumark-legacy（本地适配层）** — MIT — 仅保留 ACE/ARC/ARJ/ASAR/CAB/CPIO/DEB/ISO/LZH/UDF 读取器和共享 trait
- **reqwest** — MIT OR Apache-2.0 — https://github.com/seanmonstar/reqwest — OAuth2 刷新令牌请求
- **enc_file** — MIT OR Apache-2.0 — https://github.com/ArdentEmpiricist/enc_file — AES 加密容器解密
- **wim-format** — LGPL-2.1-or-later OR GPL-3.0-or-later — https://github.com/CaddyGlow/wim-rs — WIM LZMS/SWM 只读解析；按 LGPL 方式静态链接，源码可从上游或本项目获取
- **ms-compress** — LGPL-2.1-only AND LGPL-2.1-or-later AND Zlib AND Apache-2.0 AND MIT — https://github.com/CaddyGlow/ms-compress — wim-format 的 XPRESS/LZX/LZMS 解码后端
- **wim** — LGPL-2.1-or-later OR GPL-3.0-or-later — https://github.com/CaddyGlow/wim-rs — WIM LZX 压缩创建
- **Brotli** — MIT — https://github.com/dropbox/rust-brotli
- **lz4_flex** — MIT — https://github.com/pseitz/lz4_flex
- **winreg** — MIT — https://github.com/gentoo90/winreg-rs
- **windows-sys / windows / windows-core** — MIT OR Apache-2.0 — https://github.com/microsoft/windows-rs
- **raw-window-handle** — MIT OR Apache-2.0 — https://github.com/rust-windowing/raw-window-handle
- **url** — MIT OR Apache-2.0 — https://github.com/servo/rust-url — 资源管理器 `file:` URL 转为本地路径
- **image** — MIT OR Apache-2.0 — https://github.com/image-rs/image — Windows 系统文件图标 PNG 编码
- **embed-resource** — MIT — https://github.com/nabijaczleweli/rust-embed-resource — Windows 图标资源编译（构建期）
- **unrar / unrar_sys** — UnRAR freeware license (RARLAB) — https://www.rarlab.com/rar/unrarsrc-7.2.1.tar.gz
- **zpaq_rs（vendor/zpaq_rs 本地 fork）** — CC0-1.0 — https://github.com/turtle261/zpaq-rs — 内嵌 libzpaq/zpaq.cpp 的 Rust 绑定；新增流式归档写入器，构建期需要 C++17 编译器
- **oxiarc-lzhuf** — Apache-2.0 — https://github.com/cool-japan/oxiarc — LZH LZSS 匹配与块编码逻辑；流式编码器改编自其 0.3.6 版本
- **libzpaq / zpaq.cpp** — public domain（其中 libdivsufsort-lite 部分为 MIT）— https://github.com/zpaq/zpaq
- **unarc-rs** — MIT OR Apache-2.0 — https://github.com/mkrueger/unarc-rs — 传统格式（ZOO/HA/HYP/SQ/SQZ/UC2/ICE/PACKICE/TAR.Z）只读适配
- **unalz** — MIT — https://github.com/alkegi/unalz — ALZ 只读解压
- **rust-i18n** — MIT — https://github.com/longbridge/rust-i18n — 应用与 gpui-kit 组件本地化
- **lzma-rust2** — Apache-2.0 — https://github.com/hasenbanck/lzma-rust2 — TAR.LZ/TAR.LZMA 读写
- **libchm** — MIT — CHM 只读解析
- **backhand** — MIT OR Apache-2.0 — SquashFS 只读解析
- **nsis** — Apache-2.0 — NSIS 安装包只读解析；测试夹具来自 nsis-rs，保留 `tests/fixtures/nsis/NOTICE.txt`
- **dpp / hfsplus / udif** — MIT — DMG/HFS+ 只读解析与测试镜像生成
- **msi / cab** — MIT — MSI 内嵌 CAB 只读解析和测试夹具
- **unegg** — MIT — EGG 只读解析
- **libazo（本地占位）** — MIT — AZO 解码占位；AZO 条目明确返回不支持
- **vendor/xz2（本地适配层）** — MIT OR Apache-2.0 — 将 `geezipx-core` 的 XZ/LZMA 调用转到 `lzma-rust2`
- **liblzma / liblzma-sys** — public domain / MIT — `backhand` 的 SquashFS XZ 后端；仅构建期链接
- **Inno Setup 6.7.3（`installer/tools/innosetup` 内置编译器）** — Inno Setup License — https://jrsoftware.org/ — Windows 安装器构建工具；生成的安装器包含其运行时，构建机不再需要安装 Inno Setup。商业使用请阅读上游商业许可说明。

- **deflate64** — MIT — https://github.com/anatawa12/deflate64-rs — ZIP Deflate64 只读解码
- **snap** — BSD-3-Clause — https://github.com/BurntSushi/rust-snappy — Snappy Framed 单文件流读写
- **bzip3-rs** — LGPL-3.0-only AND Apache-2.0 — https://github.com/ing-systems/bzip3-rs — BZip3 单文件流读写；静态链接，合规说明见 `docs/lgpl-compliance.md`
- **bitstream-io** — MIT OR Apache-2.0 — https://github.com/tuffy/bitstream-io — ZIP 旧方法（Shrink/Reduce/Implode）只读解码
- **Lucide icons** (through gpui-kit assets) — ISC — https://lucide.dev
- **IconPark** — Apache-2.0 — https://iconpark.oceanengine.com/official — selected SVG assets are stored under `crates/lumark-ui-shared/assets/icons/iconpark/` with the upstream license.
- **内置伪装面具媒体** — `crates/lumark-archive/assets/disguise/cover.mp4` 和 `cover.jpg` 由本项目使用本地 ffmpeg 生成（渐变画面，无第三方素材），以 CC0 方式随项目分发；ffmpeg 仅用于生成资源，不随安装包分发。
- **rippod/apate** — GPL-3.0 — https://github.com/rippod/apate — 仅作为“文件伪装/一键还原”功能形态的参考，未复用其代码、二进制或容器格式。
- **trumank/repak** — MIT OR Apache-2.0 — https://github.com/trumank/repak — Unreal PAK 版本/索引/加密/压缩行为参考，测试样本位于 `crates/lumark-archive/tests/fixtures/unreal/`（含 NOTICE），未直接链接其代码。
- **trumank/retoc** — MIT — https://github.com/trumank/retoc — IoStore TOC/目录索引/chunk 布局与 UE5 Zen 包格式参考；Lumark 独立实现，未直接链接其代码。
- **CUE4Parse** — Apache-2.0 — https://github.com/FabianFG/CUE4Parse — UE5 Zen/无版本属性/unversioned header/纹理平台数据行为参考；Lumark 按其公开格式独立实现，测试用 UE5.8 fixture 仅本地验证。
- **unrealfs** — MIT — https://github.com/lmmtrr/unrealfs — `.usmap` 文件格式和属性 schema 解析参考。
- **texture2ddecoder** — MIT OR Apache-2.0 — https://github.com/UniversalGameExtraction/texture2ddecoder — BC1/BC3/BC4/BC5/BC7 软件解码。
- **yuhkix/aesdumpster-rs** — MIT — https://github.com/yuhkix/aesdumpster-rs — `C7` AES 密钥签名模式、误报表与熵排序参考；Lumark 独立实现并增加真实容器校验。
- **GHFear/AESDumpster** — 参考项目 — https://github.com/GHFear/AESDumpster — AES 密钥提取算法来源，未复用其代码或二进制。
- **4sval/FModel** — GPL-3.0 — https://github.com/4sval/FModel — 仅作为游戏资产浏览、目录选择、容器/包/查看器功能语义的参考；未复制其代码、界面或资源，Lumark 的 UI 与实现独立完成。
- **oozextract** — MIT — https://crates.io/crates/oozextract / https://github.com/lvlvllvlvllvlvl/oozextract — 纯 Rust 的 Oodle 解压实现（Kraken/Mermaid/Selkie/Leviathan/LZNA/Bitknit/LZB16），移植自 powzix/ooz；Lumark 默认用它解压 Oodle 压缩块。上游 `powzix/ooz` 仓库未声明独立许可证，再分发前请自行复核该许可链。
- **Oodle（可选 DLL 后端）** — Oodle 压缩格式与官方 `OodleLZ_Decompress` 属于 Epic Games/RAD Game Tools；Lumark 不包含、不随包分发官方 Oodle 库。仅当用户显式提供游戏自带的 `oo2core*.dll`、`oodle-data-shared.dll` 或设置 `LUMARK_OODLE_DLL` 时才会动态加载，相关许可由用户与权利人另行约定。

RAR support is read-only. The embedded UnRAR source and its license must be included in release artifacts; no RAR creation is implemented.

`crates/lumark-archive/tests/fixtures/` contains small read-only test samples copied from the MIT-licensed
[zip-rs/zip2](https://github.com/zip-rs/zip2), the MIT OR Apache-2.0 licensed
[unarc-rs](https://github.com/mkrueger/unarc-rs) test corpora, and the MIT OR Apache-2.0 licensed
[muja/unrar.rs](https://github.com/muja/unrar.rs) RAR5 corpus (`tests/fixtures/rar5/NOTICE.txt`) and the
MIT OR Apache-2.0 licensed trumank/repak Unreal PAK corpus
(`crates/lumark-archive/tests/fixtures/unreal/NOTICE.txt`). They are used only by tests and are not part of
release binaries.
