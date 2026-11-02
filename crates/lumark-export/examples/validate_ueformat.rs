//! 手工校验 UEFormat UEMODEL 文件头与根属性。
//!
//! 用法：`validate_ueformat <file.uemodel>`

use std::error::Error;

/// 属性名与属性数据。
type Attributes = Vec<(String, Vec<u8>)>;

/// 运行校验。
fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args().nth(1).ok_or("缺少 .uemodel 路径")?;
    let data = std::fs::read(&path)?;
    let mut reader = Reader::new(&data);
    if reader.bytes(8) != b"UEFORMAT" {
        return Err("magic 不是 UEFORMAT".into());
    }
    let identifier = reader.fstring();
    let version = reader.u8();
    let object_name = reader.fstring();
    let object_path = reader.fstring();
    let compressed = reader.u8();
    println!(
        "uemodel={path} id={identifier} version={version} name={object_name} path={object_path} compressed={compressed}"
    );
    if compressed != 0 {
        return Err("暂不支持压缩的 UEFormat".into());
    }
    let root = attributes(&mut reader)?;
    println!(
        "root_attributes={:?}",
        root.iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
    );
    for (name, body) in &root {
        if name == "LODS" {
            let mut lod_reader = Reader::new(body);
            let count = lod_reader.i32();
            println!("lods={count}");
            for _ in 0..count.max(0) {
                let lod_name = lod_reader.fstring();
                let lod_attributes = attributes(&mut lod_reader)?;
                let names: Vec<&str> = lod_attributes
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect();
                println!("  lod={lod_name} attributes={names:?}");
                for (attribute, data) in &lod_attributes {
                    if matches!(attribute.as_str(), "VERTICES" | "INDICES" | "MATERIALS") {
                        let count = i32::from_le_bytes(data[..4].try_into().unwrap_or_default());
                        println!("    {attribute} count={count}");
                    }
                }
            }
        } else if name == "SKELETON" {
            let mut skeleton_reader = Reader::new(body);
            let skeleton_attributes = attributes(&mut skeleton_reader)?;
            let names: Vec<&str> = skeleton_attributes
                .iter()
                .map(|(name, _)| name.as_str())
                .collect();
            println!("skeleton_attributes={names:?}");
            if let Some((_, bones)) = skeleton_attributes.iter().find(|(name, _)| name == "BONES") {
                let count = i32::from_le_bytes(bones[..4].try_into().unwrap_or_default());
                println!("  bones={count}");
            }
        }
    }
    Ok(())
}

/// 小端读取器。
struct Reader<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, position: 0 }
    }

    fn bytes(&mut self, count: usize) -> &'a [u8] {
        let end = (self.position + count).min(self.data.len());
        let slice = &self.data[self.position..end];
        self.position = end;
        slice
    }

    fn u8(&mut self) -> u8 {
        self.bytes(1)[0]
    }

    fn i32(&mut self) -> i32 {
        i32::from_le_bytes(self.bytes(4).try_into().unwrap_or_default())
    }

    fn fstring(&mut self) -> String {
        let length = self.i32().max(0) as usize;
        String::from_utf8_lossy(self.bytes(length)).to_string()
    }
}

/// 读取属性集。
fn attributes(reader: &mut Reader) -> Result<Attributes, Box<dyn Error>> {
    let count = reader.i32();
    if count < 0 {
        return Err("属性数量为负".into());
    }
    let mut result = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let name = reader.fstring();
        let length = reader.i32();
        if length < 0 {
            return Err(format!("属性 {name} 长度为负").into());
        }
        let data = reader.bytes(length as usize).to_vec();
        result.push((name, data));
    }
    Ok(result)
}
