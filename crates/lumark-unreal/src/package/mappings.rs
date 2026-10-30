//! `.usmap` 映射文件读取。
//!
//! 无版本属性（Zen/UE5 cooked package 常见）没有属性名标签，需要社区维护的
//! `.usmap` 提供类层级、属性顺序和类型。本实现参考 MIT 许可的 unrealfs 的
//! 公开格式解析逻辑，按 Lumark 的错误模型重写，不链接其代码。
//!
//! 支持 USMAP v1–v4、未压缩 / ZStandard / Brotli 载荷；Oodle 载荷返回
//! 可操作错误（通常导出为 ZStandard 即可）。

use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::Path;

use super::{PackageError, invalid};

/// `.usmap` 魔数。
const MAGIC: u16 = 0x30C4;
/// 载荷字节上限。
const MAX_PAYLOAD: usize = 512 * 1024 * 1024;

/// 映射中的属性类型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropType {
    /// 字节。
    Byte,
    /// 布尔。
    Bool,
    /// 8 位整数。
    Int8,
    /// 16 位整数。
    Int16,
    /// 32 位整数。
    Int,
    /// 64 位整数。
    Int64,
    /// 16 位无符号整数。
    UInt16,
    /// 32 位无符号整数。
    UInt32,
    /// 64 位无符号整数。
    UInt64,
    /// 单精度浮点。
    Float,
    /// 双精度浮点。
    Double,
    /// 对象引用。
    Object,
    /// 名称。
    Name,
    /// 委托。
    Delegate,
    /// 多播委托。
    MulticastDelegate,
    /// 动态数组。
    Array(Box<PropType>),
    /// 集合。
    Set(Box<PropType>),
    /// 映射。
    Map(Box<PropType>, Box<PropType>),
    /// 可选值。
    Optional(Box<PropType>),
    /// 自定义结构。
    Struct(String),
    /// 枚举。
    Enum(Box<PropType>, String),
    /// 字符串。
    Str,
    /// UTF-8 字符串。
    Utf8Str,
    /// ANSI 字符串。
    AnsiStr,
    /// 文本。
    Text,
    /// 接口。
    Interface,
    /// 弱对象。
    WeakObject,
    /// 懒加载对象。
    LazyObject,
    /// 软对象路径。
    SoftObject,
    /// 字段路径。
    FieldPath,
    /// 未知类型。
    Unknown,
}

/// 一个属性的 schema。
#[derive(Debug, Clone)]
pub struct PropSchema {
    /// 属性名。
    pub name: String,
    /// 在类层级中的槽位。
    pub schema_index: u16,
    /// 数组/静态数组大小。
    pub array_size: u8,
    /// 类型。
    pub ty: PropType,
}

/// 一个结构/类的 schema。
#[derive(Debug, Clone)]
pub struct StructSchema {
    /// 父类名称。
    pub super_name: Option<String>,
    /// 属性槽位总数（含继承）。
    pub prop_count: u16,
    /// 可序列化属性。
    pub props: Vec<PropSchema>,
}

/// 一个 `.usmap` 映射。
#[derive(Debug, Default, Clone)]
pub struct Mappings {
    /// 结构/类 schema。
    pub structs: HashMap<String, StructSchema>,
    /// 枚举值和名称。
    pub enums: HashMap<String, Vec<(i64, String)>>,
}

impl Mappings {
    /// 从文件读取映射。
    pub fn parse_file(path: &Path) -> Result<Self, PackageError> {
        let data = std::fs::read(path)?;
        Self::parse(&data)
    }

    /// 从字节读取映射。
    pub fn parse(data: &[u8]) -> Result<Self, PackageError> {
        let mut reader = MapReader::new(data);
        if reader.u16()? != MAGIC {
            return Err(invalid("不是 .usmap 文件（魔数不匹配）"));
        }
        let version = reader.u8()?;
        if version > 4 {
            return Err(PackageError::Unsupported(format!(
                "不支持的 .usmap 版本 {version}"
            )));
        }
        if version >= 1 && reader.bool32()? {
            let _package_version = reader.u64()?;
            let custom_versions = reader.count(20)?;
            reader.skip(custom_versions * 20 + 4)?;
        }
        let method = reader.u8()?;
        let compressed_size = reader.u32()? as usize;
        let size = reader.u32()? as usize;
        if compressed_size > MAX_PAYLOAD || size > MAX_PAYLOAD {
            return Err(invalid(".usmap 载荷超过 512 MiB 安全上限"));
        }
        let payload = reader.bytes(compressed_size)?;
        let body = match method {
            0 => payload.to_vec(),
            2 => {
                let mut out = Vec::with_capacity(size);
                brotli::Decompressor::new(payload, 4096)
                    .read_to_end(&mut out)
                    .map_err(|error| invalid(format!(".usmap Brotli 解压失败：{error}")))?;
                out
            }
            3 => {
                let mut out = Vec::with_capacity(size);
                zstd::stream::read::Decoder::new(payload)
                    .map_err(|error| invalid(format!(".usmap ZStandard 解压失败：{error}")))?
                    .read_to_end(&mut out)
                    .map_err(|error| invalid(format!(".usmap ZStandard 解压失败：{error}")))?;
                out
            }
            1 => {
                return Err(PackageError::Unsupported(
                    "Oodle 压缩的 .usmap 暂不支持；请导出为 ZStandard 或未压缩".to_string(),
                ));
            }
            other => {
                return Err(PackageError::Unsupported(format!(
                    "未知的 .usmap 压缩方法 {other}"
                )));
            }
        };
        if body.len() != size {
            return Err(invalid(format!(
                ".usmap 解压后长度 {} 与声明 {size} 不一致",
                body.len()
            )));
        }
        Body {
            reader: MapReader::new(&body),
            version,
            names: Vec::new(),
        }
        .read()
    }

    /// 查找结构/类 schema。
    pub fn find(&self, name: &str) -> Option<&StructSchema> {
        self.structs.get(name)
    }

    /// 解析枚举显示名。
    pub fn enum_name(&self, enum_name: &str, value: i64) -> Option<&str> {
        self.enums
            .get(enum_name)?
            .iter()
            .find(|(entry_value, _)| *entry_value == value)
            .map(|(_, name)| name.as_str())
    }

    /// 按类层级解析一个属性槽位。
    pub fn property_at(&self, struct_name: &str, mut index: usize) -> Option<(&PropSchema, usize)> {
        let mut chain = Vec::new();
        let mut current = Some(struct_name);
        while let Some(name) = current {
            let Some(schema) = self.structs.get(name) else {
                if chain.is_empty() {
                    return None;
                }
                break;
            };
            chain.push(schema);
            current = schema.super_name.as_deref();
        }
        for schema in chain {
            let count = schema.prop_count as usize;
            if index < count {
                return schema.props.iter().find_map(|property| {
                    let start = property.schema_index as usize;
                    let size = property.array_size.max(1) as usize;
                    (index >= start && index < start + size).then_some((property, index - start))
                });
            }
            index -= count;
        }
        None
    }
}

/// USMAP 载荷解析器。
struct Body<'a> {
    reader: MapReader<'a>,
    version: u8,
    names: Vec<String>,
}

impl Body<'_> {
    /// 读取名称索引。
    fn name(&mut self) -> Result<String, PackageError> {
        let index = self.reader.i32()?;
        if index < 0 {
            return Ok(String::new());
        }
        self.names
            .get(index as usize)
            .cloned()
            .ok_or_else(|| invalid(format!(".usmap 名称索引 {index} 越界")))
    }

    /// 读取属性类型。
    fn prop_type(&mut self) -> Result<PropType, PackageError> {
        let id = self.reader.u8()?;
        Ok(match id {
            0 => PropType::Byte,
            1 => PropType::Bool,
            2 => PropType::Int,
            3 => PropType::Float,
            4 => PropType::Object,
            5 => PropType::Name,
            6 => PropType::Delegate,
            7 => PropType::Double,
            8 => PropType::Array(Box::new(self.prop_type()?)),
            9 => PropType::Struct(self.name()?),
            10 => PropType::Str,
            11 => PropType::Text,
            12 => PropType::Interface,
            13 => PropType::MulticastDelegate,
            14 => PropType::WeakObject,
            15 => PropType::LazyObject,
            16 | 17 => PropType::SoftObject,
            18 => PropType::UInt64,
            19 => PropType::UInt32,
            20 => PropType::UInt16,
            21 => PropType::Int64,
            22 => PropType::Int16,
            23 => PropType::Int8,
            24 => {
                let key = self.prop_type()?;
                PropType::Map(Box::new(key), Box::new(self.prop_type()?))
            }
            25 => PropType::Set(Box::new(self.prop_type()?)),
            26 => {
                let inner = self.prop_type()?;
                PropType::Enum(Box::new(inner), self.name()?)
            }
            27 => PropType::FieldPath,
            28 => PropType::Optional(Box::new(self.prop_type()?)),
            29 => PropType::Utf8Str,
            30 => PropType::AnsiStr,
            255 => PropType::Unknown,
            other => {
                return Err(PackageError::Unsupported(format!(
                    "未知的 .usmap 属性类型 {other}"
                )));
            }
        })
    }

    /// 读取名称表、枚举和结构。
    fn read(mut self) -> Result<Mappings, PackageError> {
        let name_count = self.reader.count(1)?;
        for _ in 0..name_count {
            let length = if self.version >= 2 {
                self.reader.u16()? as usize
            } else {
                self.reader.u8()? as usize
            };
            self.names
                .push(String::from_utf8_lossy(self.reader.bytes(length)?).into_owned());
        }
        let mut mappings = Mappings::default();
        let enum_count = self.reader.u32()? as usize;
        for _ in 0..enum_count {
            let name = self.name()?;
            let entries = if self.version >= 3 {
                self.reader.u16()? as usize
            } else {
                self.reader.u8()? as usize
            };
            let mut values = Vec::with_capacity(entries);
            for index in 0..entries {
                let value = if self.version >= 4 {
                    self.reader.i64()?
                } else {
                    index as i64
                };
                let entry = self.name()?;
                let entry = entry.rsplit("::").next().unwrap_or(&entry).to_string();
                values.push((value, entry));
            }
            mappings.enums.insert(name, values);
        }
        let struct_count = self.reader.u32()? as usize;
        for _ in 0..struct_count {
            let name = self.name()?;
            let super_name = Some(self.name()?).filter(|name| !name.is_empty());
            let prop_count = self.reader.u16()?;
            let serializable = self.reader.u16()? as usize;
            let mut props = Vec::with_capacity(serializable);
            for _ in 0..serializable {
                let schema_index = self.reader.u16()?;
                let array_size = self.reader.u8()?;
                let property_name = self.name()?;
                props.push(PropSchema {
                    name: property_name,
                    schema_index,
                    array_size,
                    ty: self.prop_type()?,
                });
            }
            mappings.structs.insert(
                name,
                StructSchema {
                    super_name,
                    prop_count,
                    props,
                },
            );
        }
        Ok(mappings)
    }
}

/// 映射文件读取游标。
struct MapReader<'a> {
    cursor: Cursor<&'a [u8]>,
}

impl<'a> MapReader<'a> {
    /// 创建读取器。
    fn new(data: &'a [u8]) -> Self {
        Self {
            cursor: Cursor::new(data),
        }
    }

    /// 读取指定长度。
    fn bytes(&mut self, length: usize) -> Result<&'a [u8], PackageError> {
        let start = self.cursor.position() as usize;
        let end = start
            .checked_add(length)
            .ok_or_else(|| invalid(".usmap 读取长度溢出"))?;
        let data = self.cursor.get_ref();
        let slice = data
            .get(start..end)
            .ok_or_else(|| invalid(".usmap 数据被截断"))?;
        self.cursor.set_position(end as u64);
        Ok(slice)
    }

    /// 跳过指定长度。
    fn skip(&mut self, length: usize) -> Result<(), PackageError> {
        let _ = self.bytes(length)?;
        Ok(())
    }

    /// 读取 u8。
    fn u8(&mut self) -> Result<u8, PackageError> {
        Ok(self.bytes(1)?[0])
    }

    /// 读取 u16。
    fn u16(&mut self) -> Result<u16, PackageError> {
        Ok(u16::from_le_bytes(
            self.bytes(2)?.try_into().unwrap_or_default(),
        ))
    }

    /// 读取 u32。
    fn u32(&mut self) -> Result<u32, PackageError> {
        Ok(u32::from_le_bytes(
            self.bytes(4)?.try_into().unwrap_or_default(),
        ))
    }

    /// 读取 i32。
    fn i32(&mut self) -> Result<i32, PackageError> {
        Ok(i32::from_le_bytes(
            self.bytes(4)?.try_into().unwrap_or_default(),
        ))
    }

    /// 读取 i64。
    fn i64(&mut self) -> Result<i64, PackageError> {
        Ok(i64::from_le_bytes(
            self.bytes(8)?.try_into().unwrap_or_default(),
        ))
    }

    /// 读取 u64。
    fn u64(&mut self) -> Result<u64, PackageError> {
        Ok(u64::from_le_bytes(
            self.bytes(8)?.try_into().unwrap_or_default(),
        ))
    }

    /// 读取 32 位布尔。
    fn bool32(&mut self) -> Result<bool, PackageError> {
        match self.u32()? {
            0 => Ok(false),
            1 => Ok(true),
            value => Err(invalid(format!(".usmap 布尔值非法：{value}"))),
        }
    }

    /// 读取带最小元素大小校验的数组数量。
    fn count(&mut self, min_element_size: usize) -> Result<usize, PackageError> {
        let count = self.i32()?;
        if count < 0
            || (count as usize).saturating_mul(min_element_size.max(1))
                > self.cursor.get_ref().len()
        {
            return Err(invalid(format!(".usmap 数组数量非法：{count}")));
        }
        Ok(count as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造最小未压缩 USMAP：一个 `Texture2D` 结构，包含一个 Int 属性。
    fn minimal_usmap() -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&2u32.to_le_bytes());
        for name in ["Texture2D", "SizeX"] {
            body.extend_from_slice(&(name.len() as u16).to_le_bytes());
            body.extend_from_slice(name.as_bytes());
        }
        body.extend_from_slice(&0u32.to_le_bytes()); // enums
        body.extend_from_slice(&1u32.to_le_bytes()); // structs
        body.extend_from_slice(&0i32.to_le_bytes()); // "Texture2D"
        body.extend_from_slice(&(-1i32).to_le_bytes()); // super ""
        body.extend_from_slice(&1u16.to_le_bytes()); // prop count
        body.extend_from_slice(&1u16.to_le_bytes()); // serializable count
        body.extend_from_slice(&0u16.to_le_bytes()); // schema index
        body.push(1); // array size
        body.extend_from_slice(&1i32.to_le_bytes()); // "SizeX"
        body.push(2); // Int
        let mut file = Vec::new();
        file.extend_from_slice(&MAGIC.to_le_bytes());
        file.push(2); // version
        file.extend_from_slice(&0u32.to_le_bytes()); // no versioning info
        file.push(0); // no compression
        file.extend_from_slice(&(body.len() as u32).to_le_bytes());
        file.extend_from_slice(&(body.len() as u32).to_le_bytes());
        file.extend_from_slice(&body);
        file
    }

    /// 解析最小映射并按槽位查找属性。
    #[test]
    fn parses_minimal_usmap() {
        let mappings = Mappings::parse(&minimal_usmap()).unwrap();
        let schema = mappings.find("Texture2D").unwrap();
        assert_eq!(schema.props.len(), 1);
        let (property, element) = mappings.property_at("Texture2D", 0).unwrap();
        assert_eq!(property.name, "SizeX");
        assert_eq!(property.ty, PropType::Int);
        assert_eq!(element, 0);
        assert!(Mappings::parse(b"bad").is_err());
    }
}
