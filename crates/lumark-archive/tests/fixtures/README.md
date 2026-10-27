# 归档测试样例

这些文件只用于 `lumark-archive` 的只读兼容性测试，不会打包进发布二进制。

## ZIP 兼容样例

来源：[zip-rs/zip2](https://github.com/zip-rs/zip2) 的 `tests/data`，MIT 许可。

| 文件 | 用途 | 原始地址 |
| --- | --- | --- |
| `zip/shrink.zip` | Shrink（方法 1）解压 | https://github.com/zip-rs/zip2/blob/master/tests/data/legacy/shrink.zip |
| `zip/reduce.zip` | Reduce（方法 2–5）解压 | https://github.com/zip-rs/zip2/blob/master/tests/data/legacy/reduce.zip |
| `zip/implode.zip` | Implode（方法 6）解压 | https://github.com/zip-rs/zip2/blob/master/tests/data/legacy/implode.zip |

Deflate64 没有可用的 Rust 编码器，测试中用一段普通 raw Deflate 流标记为方法 9
来验证方法分派；Deflate64 是 Deflate 的超集，该流对 Deflate64 解码器合法。

## 传统格式样例

来源：[unarc-rs 0.6.3](https://crates.io/crates/unarc-rs/0.6.3) 的 `tests/` 资源，MIT OR Apache-2.0 许可。

| 文件 | 格式 |
| --- | --- |
| `legacy/zoo/default.zoo` | ZOO |
| `legacy/ha/asc.ha` | HA |
| `legacy/hyp/license.hyp` | HYP |
| `legacy/sq/license.sq` | SQ |
| `legacy/sqz/license_m1.sqz` | SQZ（正常路径） |
| `legacy/sqz/store.sqz` | SQZ（包含 `../LICENSE`，用于路径逃逸拒绝测试） |
| `legacy/uc2/normal.uc2` | UC2 |
| `legacy/ice/license_lha.ice` | ICE |
| `legacy/pi9/KOERPER.PI9` | PACKICE |
| `legacy/tarz/license.tar.Z` | TAR.Z |

这些样例仅用于验证列表、解压和 CRC 校验，不要修改或重新压缩它们，以免破坏兼容性基线。
