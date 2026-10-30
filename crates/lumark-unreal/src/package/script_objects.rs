//! UE5 Zen 全局脚本对象表。
//!
//! 全局容器中的 ScriptObjects 块保存“全局导入哈希 -> 类/对象名称”的映射；
//! 包的导出类索引大多是脚本导入，需要这张表才能把类名解析成
//! `StaticMesh`、`Texture2D` 等可读名称。

use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;

use lumark_archive::IoStoreContainer;

use super::name_map::NameMap;
use super::{PackageError, invalid, read_u32, read_u64};

/// 脚本对象表中的一项。
#[derive(Debug, Clone)]
pub struct ScriptObject {
    /// 对象名称。
    pub name: String,
    /// 外层对象索引。
    pub outer_index: u64,
    /// 类默认对象的类索引。
    pub class_index: u64,
}

/// 全局脚本对象表。
#[derive(Debug, Default)]
pub struct ZenScriptObjects {
    entry_count: usize,
    names: NameMap,
    lookup: HashMap<u64, ScriptObject>,
}

impl ZenScriptObjects {
    /// 解析 ScriptObjects 块的字节流。
    pub fn parse(data: &[u8]) -> Result<Self, PackageError> {
        let mut cursor = Cursor::new(data);
        let names = NameMap::read(&mut cursor)?;
        let count = read_u32(&mut cursor)? as usize;
        if count > 1_000_000 {
            return Err(invalid("脚本对象数量超过安全上限"));
        }
        let mut lookup = HashMap::with_capacity(count);
        for _ in 0..count {
            let object_name = NameMap::read_name(&mut cursor)?;
            let global_index = read_u64(&mut cursor)?;
            let outer_index = read_u64(&mut cursor)?;
            let class_index = read_u64(&mut cursor)?;
            let name = names.get(object_name)?;
            lookup.insert(
                global_index,
                ScriptObject {
                    name,
                    outer_index,
                    class_index,
                },
            );
        }
        Ok(Self {
            entry_count: count,
            names,
            lookup,
        })
    }

    /// 从全局 IoStore 容器加载脚本对象。
    ///
    /// `key` 仅在全局容器加密时需要；通常 `global.utoc` 未加密。
    pub fn from_container(path: &Path, key: Option<&str>) -> Result<Self, PackageError> {
        let container = IoStoreContainer::open(path, key)?;
        let Some(index) = container.find_chunks_by_type(5).into_iter().next() else {
            return Err(PackageError::Unsupported(
                "全局容器中没有 ScriptObjects 块".to_string(),
            ));
        };
        Self::parse(&container.read_chunk(index)?)
    }

    /// 按全局索引解析类/对象名称。
    pub fn resolve_name(&self, index: u64) -> Option<&str> {
        self.lookup.get(&index).map(|entry| entry.name.as_str())
    }

    /// 返回脚本对象条目。
    pub fn get(&self, index: u64) -> Option<&ScriptObject> {
        self.lookup.get(&index)
    }

    /// 返回脚本对象数量。
    pub fn len(&self) -> usize {
        self.entry_count
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entry_count == 0
    }

    /// 返回全局名称表中给定索引的名称（用于调试/后续解析）。
    pub fn global_name(&self, index: usize) -> Option<String> {
        self.names.name_at(index).map(str::to_string)
    }

    /// 返回全局名称数量。
    pub fn global_name_count(&self) -> usize {
        self.names.len()
    }
}
