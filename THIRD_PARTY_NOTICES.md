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

- **deflate64** — MIT — https://github.com/anatawa12/deflate64-rs — ZIP Deflate64 只读解码
- **bitstream-io** — MIT OR Apache-2.0 — https://github.com/tuffy/bitstream-io — ZIP 旧方法（Shrink/Reduce/Implode）只读解码
- **Lucide icons** (through gpui-kit assets) — ISC — https://lucide.dev
- **IconPark** — Apache-2.0 — https://iconpark.oceanengine.com/official — selected SVG assets are stored under `crates/lumark-ui/assets/icons/iconpark/` with the upstream license.
- **内置伪装面具媒体** — `crates/lumark-archive/assets/disguise/cover.mp4` 和 `cover.jpg` 由本项目使用本地 ffmpeg 生成（渐变画面，无第三方素材），以 CC0 方式随项目分发；ffmpeg 仅用于生成资源，不随安装包分发。
- **rippod/apate** — GPL-3.0 — https://github.com/rippod/apate — 仅作为“文件伪装/一键还原”功能形态的参考，未复用其代码、二进制或容器格式。

RAR support is read-only. The embedded UnRAR source and its license must be included in release artifacts; no RAR creation is implemented.

`crates/lumark-archive/tests/fixtures/` contains small read-only test samples copied from the MIT-licensed
[zip-rs/zip2](https://github.com/zip-rs/zip2) and the MIT OR Apache-2.0 licensed
[unarc-rs](https://github.com/mkrueger/unarc-rs) test corpora. They are used only by tests and are not part of
release binaries.
