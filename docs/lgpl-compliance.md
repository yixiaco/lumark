# BZip3 / LGPL 合规说明

Lumark 的 `.bz3` 单文件流读写使用纯 Rust crate **bzip3-rs 1.0.1**（LGPL-3.0-only AND Apache-2.0，上游：<https://github.com/ing-systems/bzip3-rs>）。

## 使用方式

- Lumark 以 Rust 静态链接方式使用 `bzip3-rs`，不修改其源代码。
- `bzip3-rs` 仅负责 BZip3 编解码；Lumark 的其余代码仍按项目自身的 MIT OR Apache-2.0 发布。
- 本项目不限制用户替换/重新链接 `bzip3-rs` 的能力；需要重新链接时，使用对应版本的 `bzip3-rs` 源码重新构建 Lumark 即可。
- 发布 Windows 安装包时，`licenses/LGPL-3.0-only.txt`、本说明和 `THIRD_PARTY_NOTICES.md` 随安装包/发布目录提供。
- `bzip3-rs` 的完整源码可从 crates.io（<https://crates.io/crates/bzip3-rs/1.0.1>）或上游仓库获取；Lumark 的构建脚本和 `Cargo.lock` 固定了使用的版本。

## 边界

- 仅支持 `.bz3` 单文件流；不支持 `.tar.bz3`、密码或分卷。
- 多文件压缩时界面回退到 ZIP，避免生成没有目录结构的单文件流。
- 若未来修改 `bzip3-rs` 本身，必须继续按 LGPL-3.0-only 提供修改后的源代码。

## 相关文件

- `LICENSE` / `LICENSE-APACHE`：Lumark 自身许可
- `THIRD_PARTY_NOTICES.md`：第三方组件清单
- `licenses/LGPL-3.0-only.txt`：LGPL-3.0 许可证文本
