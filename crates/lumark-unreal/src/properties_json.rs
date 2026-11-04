//! 资产无版本属性 → JSON 文本。
//!
//! FModel 的 JSON 页展示包导出表与每个导出的属性树；Lumark 用同一数据源
//! 生成只读 JSON：包/主导出信息、导出表、可解析的属性树。对象引用在能解析
//! 时写成名称，解析不到时保留 `{ "ObjectIndex": -2 }`，避免丢失索引。

use serde_json::{Map, Value};

use crate::package::{LegacyPackage, Mappings, PropValue, UnrealPackage};

/// JSON 文本上限（4 MB）。
const MAX_JSON_BYTES: usize = 4 * 1024 * 1024;
/// 最多展开的导出数，避免超大包拖慢预览。
const MAX_EXPORTS: usize = 64;
/// 属性树最大递归深度。
const MAX_DEPTH: usize = 64;

/// 生成 Zen 包主导出的属性 JSON。
pub fn zen_properties_json(package: &UnrealPackage, mappings: &Mappings, primary: usize) -> String {
    let info = package.info();
    let primary_export = info.exports.get(primary).or_else(|| info.exports.first());
    let mut root = Map::new();
    root.insert(
        "Package".to_string(),
        Value::String(info.package_name.clone()),
    );
    root.insert(
        "Name".to_string(),
        Value::String(
            primary_export
                .map(|export| export.name.clone())
                .unwrap_or_default(),
        ),
    );
    root.insert(
        "Class".to_string(),
        Value::String(
            primary_export
                .map(|export| export.class_name.clone())
                .unwrap_or_default(),
        ),
    );
    let resolve = |index: i32| resolve_zen_object(info, index);
    let mut exports = Vec::new();
    let mut truncated = false;
    for (index, export) in info.exports.iter().take(MAX_EXPORTS).enumerate() {
        let mut item = Map::new();
        item.insert("Index".to_string(), Value::from(index));
        item.insert("Type".to_string(), Value::String(export.class_name.clone()));
        item.insert("Name".to_string(), Value::String(export.name.clone()));
        item.insert(
            "Outer".to_string(),
            export.outer_export.map(Value::from).unwrap_or(Value::Null),
        );
        match package.read_export_properties(index, mappings) {
            Ok(properties) => {
                item.insert(
                    "Properties".to_string(),
                    properties_to_json(&properties.properties, &resolve, 0),
                );
            }
            Err(error) => {
                item.insert("Properties".to_string(), Value::Null);
                item.insert(
                    "PropertyError".to_string(),
                    Value::String(error.to_string()),
                );
            }
        }
        exports.push(Value::Object(item));
        let projected = Value::Object(root.clone()).to_string().len()
            + exports
                .iter()
                .map(|export| export.to_string().len())
                .sum::<usize>();
        if projected > MAX_JSON_BYTES {
            exports.pop();
            truncated = true;
            break;
        }
    }
    if info.exports.len() > exports.len() {
        truncated = true;
    }
    root.insert("Exports".to_string(), Value::Array(exports));
    root.insert("Truncated".to_string(), Value::Bool(truncated));
    pretty(Value::Object(root))
}

/// 生成旧式包主导出的属性 JSON。
pub fn legacy_properties_json(
    package: &LegacyPackage,
    mappings: &Mappings,
    primary: usize,
) -> String {
    let exports = package.exports();
    let primary_export = exports.get(primary).or_else(|| exports.first());
    let mut root = Map::new();
    let package_name = package
        .summary
        .export_name
        .rsplit('/')
        .next()
        .unwrap_or(&package.summary.export_name)
        .to_string();
    root.insert("Package".to_string(), Value::String(package_name));
    root.insert(
        "Name".to_string(),
        Value::String(
            primary_export
                .map(|export| export.name.clone())
                .unwrap_or_default(),
        ),
    );
    root.insert(
        "Class".to_string(),
        Value::String(
            primary_export
                .map(|export| export.class_name.clone())
                .unwrap_or_else(|| package.summary.class_name.clone()),
        ),
    );
    let resolve = |index: i32| package.resolve_object_name(index);
    let mut export_values = Vec::new();
    for (index, export) in exports.iter().take(MAX_EXPORTS).enumerate() {
        let mut item = Map::new();
        item.insert("Index".to_string(), Value::from(index));
        item.insert("Type".to_string(), Value::String(export.class_name.clone()));
        item.insert("Name".to_string(), Value::String(export.name.clone()));
        match package.read_export_properties(index, mappings) {
            Ok(properties) => {
                item.insert(
                    "Properties".to_string(),
                    properties_to_json(&properties.properties, &resolve, 0),
                );
            }
            Err(error) => {
                item.insert("Properties".to_string(), Value::Null);
                item.insert(
                    "PropertyError".to_string(),
                    Value::String(error.to_string()),
                );
            }
        }
        export_values.push(Value::Object(item));
    }
    let truncated = exports.len() > export_values.len();
    root.insert("Exports".to_string(), Value::Array(export_values));
    root.insert("Truncated".to_string(), Value::Bool(truncated));
    pretty(Value::Object(root))
}

/// 把无版本属性列表转成 JSON 对象。
pub fn properties_to_json(
    properties: &[(String, PropValue)],
    resolve: &dyn Fn(i32) -> Option<String>,
    depth: usize,
) -> Value {
    if depth >= MAX_DEPTH {
        return Value::String("<max depth>".to_string());
    }
    let mut object = Map::new();
    for (key, value) in properties {
        object.insert(key.clone(), prop_value_to_json(value, resolve, depth + 1));
    }
    Value::Object(object)
}

/// 单个属性值转 JSON。
fn prop_value_to_json(
    value: &PropValue,
    resolve: &dyn Fn(i32) -> Option<String>,
    depth: usize,
) -> Value {
    if depth >= MAX_DEPTH {
        return Value::String("<max depth>".to_string());
    }
    match value {
        PropValue::Int(value) => Value::from(*value),
        PropValue::UInt(value) => Value::from(*value),
        PropValue::Float(value) => serde_json::Number::from_f64(*value)
            .map(Value::Number)
            .unwrap_or_else(|| Value::String(format!("{value}"))),
        PropValue::Bool(value) => Value::Bool(*value),
        PropValue::Name(value) | PropValue::String(value) => Value::String(value.clone()),
        PropValue::Object(index) => match resolve(*index) {
            Some(name) => Value::String(name),
            None => {
                let mut object = Map::new();
                object.insert("ObjectIndex".to_string(), Value::from(*index));
                Value::Object(object)
            }
        },
        PropValue::Struct(fields) => properties_to_json(fields, resolve, depth + 1),
        PropValue::Array(values) | PropValue::Set(values) => Value::Array(
            values
                .iter()
                .map(|value| prop_value_to_json(value, resolve, depth + 1))
                .collect(),
        ),
        PropValue::Map(values) => Value::Array(
            values
                .iter()
                .map(|(key, value)| {
                    let mut entry = Map::new();
                    entry.insert(
                        "Key".to_string(),
                        prop_value_to_json(key, resolve, depth + 1),
                    );
                    entry.insert(
                        "Value".to_string(),
                        prop_value_to_json(value, resolve, depth + 1),
                    );
                    Value::Object(entry)
                })
                .collect(),
        ),
        PropValue::Zero => Value::from(0),
        PropValue::Other => Value::String("<raw>".to_string()),
    }
}

/// 解析 Zen 包内对象索引：正值为导出，负值为导入（导入名表不在本包内）。
fn resolve_zen_object(info: &crate::package::PackageInfo, index: i32) -> Option<String> {
    if index <= 0 {
        return None;
    }
    info.exports
        .get(index as usize - 1)
        .map(|export| export.name.clone())
}

/// 美化 JSON；序列化失败时退回紧凑文本。
fn pretty(value: Value) -> String {
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 嵌套 struct/array/map/对象索引都转换为可读 JSON。
    #[test]
    fn converts_nested_property_values() {
        let properties = vec![
            ("Name".to_string(), PropValue::Name("NanaF6_2".to_string())),
            (
                "Scale".to_string(),
                PropValue::Struct(vec![
                    ("X".to_string(), PropValue::Float(1.5)),
                    ("Y".to_string(), PropValue::Int(2)),
                ]),
            ),
            (
                "Items".to_string(),
                PropValue::Array(vec![
                    PropValue::Bool(true),
                    PropValue::Object(3),
                    PropValue::Object(-2),
                ]),
            ),
            (
                "Lookup".to_string(),
                PropValue::Map(vec![(
                    PropValue::Name("Key".to_string()),
                    PropValue::UInt(7),
                )]),
            ),
        ];
        let resolve = |index: i32| (index == 3).then(|| "ExportName".to_string());
        let value = properties_to_json(&properties, &resolve, 0);
        assert_eq!(value["Name"], "NanaF6_2");
        assert_eq!(value["Scale"]["X"], 1.5);
        assert_eq!(value["Scale"]["Y"], 2);
        assert_eq!(value["Items"][0], true);
        assert_eq!(value["Items"][1], "ExportName");
        assert_eq!(value["Items"][2]["ObjectIndex"], -2);
        assert_eq!(value["Lookup"][0]["Key"], "Key");
        assert_eq!(value["Lookup"][0]["Value"], 7);
        assert!(serde_json::from_str::<Value>(&value.to_string()).is_ok());
    }

    /// 超过深度上限时截断而不是递归爆栈。
    #[test]
    fn stops_at_max_depth() {
        let mut value = PropValue::Int(1);
        for _ in 0..(MAX_DEPTH + 4) {
            value = PropValue::Struct(vec![("Nested".to_string(), value)]);
        }
        let json = properties_to_json(&[("Root".to_string(), value)], &|_| None, 0);
        assert!(json.to_string().contains("<max depth>"));
    }

    /// 非有限浮点写成字符串，避免 JSON 序列化失败。
    #[test]
    fn writes_non_finite_float_as_string() {
        let value = properties_to_json(
            &[("Bad".to_string(), PropValue::Float(f64::NAN))],
            &|_| None,
            0,
        );
        assert!(value["Bad"].is_string());
    }
}
