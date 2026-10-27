# Third-Party Notices

Lumark is licensed under MIT OR Apache-2.0. The repository uses or references the following projects:

- **gpui-kit** — Apache-2.0 — https://github.com/longbridge/gpui-kit
- **GPUI / gpui-pre** — Apache-2.0 — https://github.com/zed-industries/zed
- **zip** — MIT — https://github.com/zip-rs/zip2
- **sevenz-rust2** — Apache-2.0 — https://github.com/hasenbanck/sevenz-rust2
- **GeeZipX** — MIT — https://github.com/GEELINX-LTD/geezipx
- **Brotli** — MIT — https://github.com/dropbox/rust-brotli
- **lz4_flex** — MIT — https://github.com/pseitz/lz4_flex
- **winreg** — MIT — https://github.com/gentoo90/winreg-rs
- **windows-sys / windows / windows-core** — MIT OR Apache-2.0 — https://github.com/microsoft/windows-rs
- **raw-window-handle** — MIT OR Apache-2.0 — https://github.com/rust-windowing/raw-window-handle
- **image** — MIT OR Apache-2.0 — https://github.com/image-rs/image — Windows 系统文件图标 PNG 编码
- **embed-resource** — MIT — https://github.com/nabijaczleweli/rust-embed-resource — Windows 图标资源编译（构建期）
- **unrar / unrar_sys** — UnRAR freeware license (RARLAB) — https://www.rarlab.com/rar/unrarsrc-7.2.1.tar.gz
- **zpaq_rs** — CC0-1.0 — https://github.com/turtle261/zpaq-rs — 内嵌 libzpaq/zpaq.cpp 的 Rust 绑定；构建期需要 C++17 编译器
- **libzpaq / zpaq.cpp** — public domain（其中 libdivsufsort-lite 部分为 MIT）— https://github.com/zpaq/zpaq
- **unarc-rs** — MIT OR Apache-2.0 — https://github.com/mkrueger/unarc-rs — 传统格式（ZOO/HA/HYP/SQ/SQZ/UC2/ICE/PACKICE/TAR.Z）只读适配
- **unalz** — MIT — https://github.com/alkegi/unalz — ALZ 只读解压
- **lzma-rust2** — Apache-2.0 — https://github.com/hasenbanck/lzma-rust2 — TAR.LZ/TAR.LZMA 读写
- **deflate64** — MIT — https://github.com/anatawa12/deflate64-rs — ZIP Deflate64 只读解码
- **bitstream-io** — MIT OR Apache-2.0 — https://github.com/tuffy/bitstream-io — ZIP 旧方法（Shrink/Reduce/Implode）只读解码
- **Lucide icons** (through gpui-kit assets) — ISC — https://lucide.dev
- **IconPark** — Apache-2.0 — https://iconpark.oceanengine.com/official — selected SVG assets are stored under `crates/lumark-ui/assets/icons/iconpark/` with the upstream license.

RAR support is read-only. The embedded UnRAR source and its license must be included in release artifacts; no RAR creation is implemented.

`crates/lumark-archive/tests/fixtures/` contains small read-only test samples copied from the MIT-licensed
[zip-rs/zip2](https://github.com/zip-rs/zip2) and the MIT OR Apache-2.0 licensed
[unarc-rs](https://github.com/mkrueger/unarc-rs) test corpora. They are used only by tests and are not part of
release binaries.
