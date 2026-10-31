//! 无版本属性（unversioned properties）读取。
//!
//! UE5 cooked 包（包括 BNB 的 Zen 包）通常不写属性名标签，而是用
//! `FUnversionedHeader` 的位图表示哪些 schema 槽位有值，再按 `.usmap`
//! 给出的类层级/类型顺序读值。本模块参考 Apache-2.0 许可的 CUE4Parse
//! 公开实现，按 Lumark 的错误模型重写。

use super::Mappings;
use super::PackageError;
use super::mappings::PropType;

/// 一个已解析的属性值。
#[derive(Debug, Clone, PartialEq)]
pub enum PropValue {
    /// 有符号整数。
    Int(i64),
    /// 无符号整数。
    UInt(u64),
    /// 浮点。
    Float(f64),
    /// 布尔。
    Bool(bool),
    /// 名称。
    Name(String),
    /// 字符串。
    String(String),
    /// 包内对象索引。
    Object(i32),
    /// 结构体字段。
    Struct(Vec<(String, PropValue)>),
    /// 数组。
    Array(Vec<PropValue>),
    /// 集合。
    Set(Vec<PropValue>),
    /// 映射。
    Map(Vec<(PropValue, PropValue)>),
    /// 零值/未序列化的占位。
    Zero,
    /// 无法进一步展开的原始值。
    Other,
}

/// 无版本属性解析结果。
pub struct UnversionedProperties {
    /// 属性列表，按 schema 顺序。
    pub properties: Vec<(String, PropValue)>,
    /// 属性流消耗的字节数。
    pub bytes_read: usize,
}

/// 从导出数据起点读取一个对象的无版本属性。
pub fn read_properties(
    data: &[u8],
    names: &[String],
    mappings: &Mappings,
    class_name: &str,
) -> Result<UnversionedProperties, PackageError> {
    read_properties_with(data, names, mappings, class_name, true)
}

/// 从旧式 UE4 导出数据起点读取无版本属性。
///
/// UE4 的 `Vector`/`Vector4` 使用 32 位浮点；UE5 使用 64 位 LWC。
pub fn read_properties_legacy(
    data: &[u8],
    names: &[String],
    mappings: &Mappings,
    class_name: &str,
) -> Result<UnversionedProperties, PackageError> {
    read_properties_with(data, names, mappings, class_name, false)
}

/// 按标量精度读取无版本属性。
fn read_properties_with(
    data: &[u8],
    names: &[String],
    mappings: &Mappings,
    class_name: &str,
    wide_scalars: bool,
) -> Result<UnversionedProperties, PackageError> {
    if mappings.find(class_name).is_none() {
        return Err(PackageError::Unsupported(format!(
            "{class_name} 不在 .usmap 映射中"
        )));
    }
    let mut reader = PropertyReader {
        data,
        position: 0,
        names,
        mappings,
        wide_scalars,
    };
    let properties = reader.read_unversioned_struct(class_name)?;
    Ok(UnversionedProperties {
        properties,
        bytes_read: reader.position,
    })
}

/// 一个 unversioned header fragment。
#[derive(Debug, Clone, Copy)]
struct Fragment {
    /// 跳过的槽位。
    skip: usize,
    /// 值的数量。
    values: usize,
    /// 是否带零值位图。
    has_zeroes: bool,
    /// 是否是最后一个 fragment。
    is_last: bool,
}

/// 属性流读取器。
struct PropertyReader<'a> {
    data: &'a [u8],
    position: usize,
    names: &'a [String],
    mappings: &'a Mappings,
    /// UE5 LWC 标量（Vector 等使用 64 位浮点）。
    wide_scalars: bool,
}

impl<'a> PropertyReader<'a> {
    /// 读取 n 字节。
    fn bytes(&mut self, length: usize) -> Result<&'a [u8], PackageError> {
        let end = self
            .position
            .checked_add(length)
            .ok_or_else(|| PackageError::Invalid("属性长度溢出".to_string()))?;
        let slice = self
            .data
            .get(self.position..end)
            .ok_or_else(|| PackageError::Invalid("属性数据被截断".to_string()))?;
        self.position = end;
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

    /// 读取 i16。
    fn i16(&mut self) -> Result<i16, PackageError> {
        Ok(i16::from_le_bytes(
            self.bytes(2)?.try_into().unwrap_or_default(),
        ))
    }

    /// 读取 u16。
    fn u16(&mut self) -> Result<u16, PackageError> {
        Ok(u16::from_le_bytes(
            self.bytes(2)?.try_into().unwrap_or_default(),
        ))
    }

    /// 读取 i32。
    fn i32(&mut self) -> Result<i32, PackageError> {
        Ok(i32::from_le_bytes(
            self.bytes(4)?.try_into().unwrap_or_default(),
        ))
    }

    /// 读取 u32。
    fn u32(&mut self) -> Result<u32, PackageError> {
        Ok(u32::from_le_bytes(
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
            value => Err(PackageError::Invalid(format!("非法布尔值 {value}"))),
        }
    }

    /// 读取 f32。
    fn f32(&mut self) -> Result<f32, PackageError> {
        Ok(f32::from_le_bytes(
            self.bytes(4)?.try_into().unwrap_or_default(),
        ))
    }

    /// 读取 f64。
    fn f64(&mut self) -> Result<f64, PackageError> {
        Ok(f64::from_le_bytes(
            self.bytes(8)?.try_into().unwrap_or_default(),
        ))
    }

    /// 读取 FName：名称索引高位是名称空间。
    fn fname(&mut self) -> Result<String, PackageError> {
        let index = self.u32()?;
        let number = self.u32()?;
        let index = (index & 0x3FFF_FFFF) as usize;
        let base = self
            .names
            .get(index)
            .cloned()
            .unwrap_or_else(|| format!("#Name{index}"));
        if number == 0 {
            Ok(base)
        } else {
            Ok(format!("{base}_{}", number - 1))
        }
    }

    /// 读取 FString。
    fn fstring(&mut self) -> Result<String, PackageError> {
        let length = self.i32()?;
        if length == 0 {
            return Ok(String::new());
        }
        if length > 0 {
            let bytes = self.bytes(length as usize)?;
            let bytes = bytes.strip_suffix(&[0]).unwrap_or(bytes);
            Ok(String::from_utf8_lossy(bytes).into_owned())
        } else {
            let units = length.unsigned_abs() as usize;
            let bytes = self.bytes(
                units
                    .checked_mul(2)
                    .ok_or_else(|| PackageError::Invalid("FString 长度溢出".to_string()))?,
            )?;
            let units = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
                .take(units.saturating_sub(1))
                .collect::<Vec<_>>();
            Ok(String::from_utf16_lossy(&units))
        }
    }

    /// 读取 unversioned header。
    fn header(&mut self) -> Result<(Vec<Fragment>, Vec<bool>), PackageError> {
        let mut fragments = Vec::new();
        let mut zero_mask_bits = 0usize;
        loop {
            let packed = self.u16()?;
            let fragment = Fragment {
                skip: (packed & 0x7F) as usize,
                values: (packed >> 9) as usize,
                has_zeroes: packed & 0x80 != 0,
                is_last: packed & 0x100 != 0,
            };
            if fragment.has_zeroes {
                zero_mask_bits += fragment.values;
            }
            fragments.push(fragment);
            if fragment.is_last {
                break;
            }
            if fragments.len() > 1_000_000 {
                return Err(PackageError::Invalid(
                    "unversioned header fragment 数量异常".to_string(),
                ));
            }
        }
        let mut zeroes = Vec::with_capacity(zero_mask_bits);
        if zero_mask_bits > 0 {
            let words = if zero_mask_bits <= 8 {
                vec![self.u8()? as u32]
            } else if zero_mask_bits <= 16 {
                vec![self.u16()? as u32]
            } else {
                (0..zero_mask_bits.div_ceil(32))
                    .map(|_| self.u32())
                    .collect::<Result<Vec<_>, _>>()?
            };
            for bit in 0..zero_mask_bits {
                zeroes.push(words[bit / 32] & (1 << (bit % 32)) != 0);
            }
        }
        Ok((fragments, zeroes))
    }

    /// 读取一个无版本结构体（对象或自定义 struct）。
    fn read_unversioned_struct(
        &mut self,
        struct_name: &str,
    ) -> Result<Vec<(String, PropValue)>, PackageError> {
        let (fragments, zeroes) = self.header()?;
        let mut zeroes = zeroes.into_iter();
        let mut properties = Vec::new();
        let mut index = 0usize;
        for fragment in fragments {
            index += fragment.skip;
            for _ in 0..fragment.values {
                let is_zero = fragment.has_zeroes && zeroes.next().unwrap_or(false);
                let (schema, element) =
                    self.mappings
                        .property_at(struct_name, index)
                        .ok_or_else(|| {
                            PackageError::Unsupported(format!(
                                "{struct_name} 的属性槽位 {index} 不在映射中"
                            ))
                        })?;
                let key = if element > 0 {
                    format!("{}[{element}]", schema.name)
                } else {
                    schema.name.clone()
                };
                let value = if is_zero {
                    zero_value(&schema.ty)
                } else {
                    self.read_value(&schema.ty).map_err(|error| {
                        PackageError::Invalid(format!("{struct_name}.{}：{error}", schema.name))
                    })?
                };
                properties.push((key, value));
                index += 1;
            }
        }
        Ok(properties)
    }

    /// 读取一个属性值。
    fn read_value(&mut self, ty: &PropType) -> Result<PropValue, PackageError> {
        Ok(match ty {
            PropType::Byte => PropValue::Int(self.u8()? as i64),
            PropType::Bool => PropValue::Bool(self.u8()? != 0),
            PropType::Int8 => PropValue::Int(self.u8()? as i8 as i64),
            PropType::Int16 => PropValue::Int(self.i16()? as i64),
            PropType::Int => PropValue::Int(self.i32()? as i64),
            PropType::Int64 => PropValue::Int(self.i64()?),
            PropType::UInt16 => PropValue::UInt(self.u16()? as u64),
            PropType::UInt32 => PropValue::UInt(self.u32()? as u64),
            PropType::UInt64 => PropValue::UInt(self.u64()?),
            PropType::Float => PropValue::Float(self.f32()? as f64),
            PropType::Double => PropValue::Float(self.f64()?),
            PropType::Name => PropValue::Name(self.fname()?),
            PropType::Object
            | PropType::Interface
            | PropType::WeakObject
            | PropType::LazyObject => PropValue::Object(self.i32()?),
            PropType::Str | PropType::Utf8Str | PropType::AnsiStr => {
                PropValue::String(self.fstring()?)
            }
            PropType::Text => PropValue::String(self.read_text()?),
            PropType::Enum(inner, enum_name) => {
                let value = self.read_value(inner)?;
                let numeric = match &value {
                    PropValue::Int(value) => Some(*value),
                    PropValue::UInt(value) => Some(*value as i64),
                    _ => None,
                };
                if let Some(numeric) = numeric
                    && let Some(name) = self.mappings.enum_name(enum_name, numeric)
                {
                    PropValue::Name(name.to_string())
                } else {
                    value
                }
            }
            PropType::Struct(name) => self.read_struct(name)?,
            PropType::Array(inner) => {
                let count = self.i32()?;
                if count < 0 || count as usize > self.data.len() {
                    return Err(PackageError::Invalid(format!("数组长度非法 {count}")));
                }
                let mut values = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    values.push(self.read_value(inner)?);
                }
                PropValue::Array(values)
            }
            PropType::Set(inner) => {
                let count = self.i32()?;
                if count < 0 || count as usize > self.data.len() {
                    return Err(PackageError::Invalid(format!("集合长度非法 {count}")));
                }
                let mut values = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    values.push(self.read_value(inner)?);
                }
                PropValue::Set(values)
            }
            PropType::Map(key, value) => {
                let count = self.i32()?;
                if count < 0 || count as usize > self.data.len() {
                    return Err(PackageError::Invalid(format!("映射长度非法 {count}")));
                }
                let mut values = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    values.push((self.read_value(key)?, self.read_value(value)?));
                }
                PropValue::Map(values)
            }
            PropType::Optional(inner) => {
                // Optional 属性先写 32 位 ReadBoolean，再写内层值。
                if self.bool32()? {
                    self.read_value(inner)?
                } else {
                    PropValue::Zero
                }
            }
            PropType::SoftObject | PropType::FieldPath => {
                return Err(PackageError::Unsupported(format!(
                    "暂不支持的属性类型 {ty:?}"
                )));
            }
            PropType::Delegate | PropType::MulticastDelegate => {
                // 委托在 cooked 包中通常只序列化对象引用。
                PropValue::Object(self.i32()?)
            }
            PropType::Unknown => {
                return Err(PackageError::Unsupported(
                    "属性类型未知，无法继续定位后续数据".to_string(),
                ));
            }
        })
    }

    /// 读取自定义/原生结构体。
    fn read_struct(&mut self, name: &str) -> Result<PropValue, PackageError> {
        match name {
            "Guid" | "GUID" => {
                let bytes = self.bytes(16)?;
                Ok(PropValue::String(
                    bytes.iter().map(|byte| format!("{byte:02X}")).collect(),
                ))
            }
            "IntPoint" | "Int32Point" => {
                let x = self.i32()?;
                let y = self.i32()?;
                Ok(PropValue::Struct(vec![
                    ("X".to_string(), PropValue::Int(x as i64)),
                    ("Y".to_string(), PropValue::Int(y as i64)),
                ]))
            }
            "PerPlatformFloat" => {
                let cooked = self.bool32()?;
                let default = self.f32()? as f64;
                if !cooked {
                    self.skip_platform_map(4)?;
                }
                Ok(PropValue::Float(default))
            }
            "PerPlatformInt" => {
                let cooked = self.bool32()?;
                let default = self.i32()? as i64;
                if !cooked {
                    self.skip_platform_map(4)?;
                }
                Ok(PropValue::Int(default))
            }
            "PerPlatformBool" => {
                let cooked = self.bool32()?;
                let default = self.bool32()?;
                if !cooked {
                    self.skip_platform_map(4)?;
                }
                Ok(PropValue::Bool(default))
            }
            "Vector" => {
                let read = |reader: &mut Self| {
                    if reader.wide_scalars {
                        reader.f64()
                    } else {
                        Ok(reader.f32()? as f64)
                    }
                };
                Ok(PropValue::Struct(vec![
                    ("X".to_string(), PropValue::Float(read(self)?)),
                    ("Y".to_string(), PropValue::Float(read(self)?)),
                    ("Z".to_string(), PropValue::Float(read(self)?)),
                ]))
            }
            "Vector3f" => Ok(PropValue::Struct(vec![
                ("X".to_string(), PropValue::Float(self.f32()? as f64)),
                ("Y".to_string(), PropValue::Float(self.f32()? as f64)),
                ("Z".to_string(), PropValue::Float(self.f32()? as f64)),
            ])),
            "LinearColor" | "Color" | "Vector4" | "Quat" | "Plane" => {
                let values = if name == "Color" {
                    let bytes = self.bytes(4)?;
                    vec![
                        bytes[2] as f64 / 255.0,
                        bytes[1] as f64 / 255.0,
                        bytes[0] as f64 / 255.0,
                        bytes[3] as f64 / 255.0,
                    ]
                } else if matches!(name, "LinearColor" | "Quat") {
                    vec![
                        self.f32()? as f64,
                        self.f32()? as f64,
                        self.f32()? as f64,
                        self.f32()? as f64,
                    ]
                } else if self.wide_scalars {
                    vec![self.f64()?, self.f64()?, self.f64()?, self.f64()?]
                } else {
                    vec![
                        self.f32()? as f64,
                        self.f32()? as f64,
                        self.f32()? as f64,
                        self.f32()? as f64,
                    ]
                };
                Ok(PropValue::Struct(vec![
                    ("X".to_string(), PropValue::Float(values[0])),
                    ("Y".to_string(), PropValue::Float(values[1])),
                    ("Z".to_string(), PropValue::Float(values[2])),
                    ("W".to_string(), PropValue::Float(values[3])),
                ]))
            }
            "BoxSphereBounds" if !self.wide_scalars => {
                let origin = self.read_struct("Vector")?;
                let extent = self.read_struct("Vector")?;
                let radius = self.f32()? as f64;
                Ok(PropValue::Struct(vec![
                    ("Origin".to_string(), origin),
                    ("BoxExtent".to_string(), extent),
                    ("SphereRadius".to_string(), PropValue::Float(radius)),
                ]))
            }
            "SkeletalMeshSamplingLODBuiltData" => {
                self.read_weighted_sampler()?;
                Ok(PropValue::Other)
            }
            "SkeletalMeshSamplingRegionBuiltData" => {
                self.skip_int_array()?; // TriangleIndices
                self.skip_int_array()?; // BoneIndices
                self.read_weighted_sampler()?;
                self.skip_int_array()?; // Vertices
                Ok(PropValue::Other)
            }
            _ => {
                if self.mappings.find(name).is_some() {
                    Ok(PropValue::Struct(self.read_unversioned_struct(name)?))
                } else {
                    Err(PackageError::Unsupported(format!(
                        "结构体 {name} 没有内置布局，也不在 .usmap 中"
                    )))
                }
            }
        }
    }

    /// 跳过 `TArray<int32>`。
    fn skip_int_array(&mut self) -> Result<(), PackageError> {
        let count = self.i32()?;
        if count < 0 || count as usize > self.data.len() / 4 {
            return Err(PackageError::Invalid(format!("数组长度非法 {count}")));
        }
        self.skip(count as usize * 4)
    }

    /// 跳过 `TMap<FName, T>`（非 cooked 的 PerPlatform 覆盖表）。
    fn skip_platform_map(&mut self, value_size: usize) -> Result<(), PackageError> {
        let count = self.i32()?;
        if count < 0 || count as usize > 100_000 {
            return Err(PackageError::Invalid(
                "PerPlatform 映射长度非法".to_string(),
            ));
        }
        for _ in 0..count {
            let _ = self.fname()?;
            self.skip(value_size)?;
        }
        Ok(())
    }

    /// 读取 `FWeightedRandomSampler`。
    fn read_weighted_sampler(&mut self) -> Result<(), PackageError> {
        let prob_count = self.i32()?;
        if prob_count < 0 || prob_count as usize > self.data.len() / 4 {
            return Err(PackageError::Invalid("采样器概率数组长度非法".to_string()));
        }
        self.skip(prob_count as usize * 4)?;
        let alias_count = self.i32()?;
        if alias_count < 0 || alias_count as usize > self.data.len() / 4 {
            return Err(PackageError::Invalid("采样器别名数组长度非法".to_string()));
        }
        self.skip(alias_count as usize * 4)?;
        let _total_weight = self.f32()?;
        Ok(())
    }

    /// 读取 FText 的常用历史类型。
    fn read_text(&mut self) -> Result<String, PackageError> {
        let _flags = self.u32()?;
        let history = self.u8()? as i8;
        match history {
            -1 => {
                if self.u32()? != 0 {
                    self.fstring()
                } else {
                    Ok(String::new())
                }
            }
            0 => {
                let _namespace = self.fstring()?;
                let _key = self.fstring()?;
                self.fstring()
            }
            other => Err(PackageError::Unsupported(format!(
                "不支持的 FText 历史类型 {other}"
            ))),
        }
    }
}

/// 零值占位。
fn zero_value(ty: &PropType) -> PropValue {
    match ty {
        PropType::Bool => PropValue::Bool(false),
        PropType::Byte
        | PropType::Int8
        | PropType::Int16
        | PropType::UInt16
        | PropType::Int
        | PropType::UInt32
        | PropType::Int64
        | PropType::UInt64 => PropValue::Int(0),
        PropType::Float | PropType::Double => PropValue::Float(0.0),
        PropType::Name => PropValue::Name("None".to_string()),
        PropType::Object | PropType::Interface | PropType::WeakObject | PropType::LazyObject => {
            PropValue::Object(0)
        }
        PropType::Str | PropType::Utf8Str | PropType::AnsiStr | PropType::Text => {
            PropValue::String(String::new())
        }
        PropType::Array(_) | PropType::Set(_) => PropValue::Array(Vec::new()),
        PropType::Map(_, _) => PropValue::Map(Vec::new()),
        PropType::Optional(_) => PropValue::Zero,
        PropType::Struct(name) => match name.as_str() {
            "Vector" | "Vector3f" => {
                PropValue::Struct(vec![("X".to_string(), PropValue::Float(0.0))])
            }
            "LinearColor" | "Color" | "Vector4" | "Quat" => {
                PropValue::Struct(vec![("X".to_string(), PropValue::Float(0.0))])
            }
            _ => PropValue::Zero,
        },
        _ => PropValue::Zero,
    }
}
