//! 手工诊断 UE4 旧式 uasset/uexp：解析摘要、名称表、导入/导出表。
//!
//! 用法：`inspect_legacy <asset.uasset> <asset.uexp>`

use std::error::Error;
use std::fs;

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
    fn i32(&mut self) -> Result<i32, String> {
        let bytes = self.take(4)?;
        Ok(i32::from_le_bytes(bytes.try_into().unwrap_or_default()))
    }
    fn u32(&mut self) -> Result<u32, String> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes(bytes.try_into().unwrap_or_default()))
    }
    fn i64(&mut self) -> Result<i64, String> {
        let bytes = self.take(8)?;
        Ok(i64::from_le_bytes(bytes.try_into().unwrap_or_default()))
    }
    fn take(&mut self, len: usize) -> Result<&'a [u8], String> {
        let end = self.pos.checked_add(len).ok_or("溢出")?;
        let bytes = self.data.get(self.pos..end).ok_or("越界")?;
        self.pos = end;
        Ok(bytes)
    }
    fn string(&mut self) -> Result<String, String> {
        let len = self.i32()?;
        if len == 0 {
            return Ok(String::new());
        }
        let negative = len < 0;
        let count = len.unsigned_abs() as usize;
        let bytes = self.take(count)?;
        if negative {
            let units = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
                .collect::<Vec<_>>();
            Ok(String::from_utf16_lossy(&units))
        } else {
            Ok(String::from_utf8_lossy(bytes)
                .trim_end_matches('\0')
                .to_string())
        }
    }
    fn fname(&mut self) -> Result<(u32, u32), String> {
        Ok((self.u32()?, self.u32()?))
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let asset = args.next().ok_or("缺少 .uasset")?;
    let uexp_path = args.next().ok_or("缺少 .uexp")?;
    let data = fs::read(&asset)?;
    let uexp = fs::read(&uexp_path)?;
    if let Ok(summary) = lumark_unreal::parse_legacy_summary(&data) {
        println!(
            "library summary: package={} class={} export={} exports={} imports={} names={}",
            summary.package_name,
            summary.class_name,
            summary.export_name,
            summary.export_count,
            summary.import_count,
            summary.name_count
        );
    }
    let mut reader = Reader::new(&data);
    let tag = reader.u32()?;
    let legacy = reader.i32()?;
    let ue3 = reader.i32()?;
    let ue4 = reader.i32()?;
    let licensee = reader.i32()?;
    let custom_count = reader.i32()?;
    if !(0..=1024).contains(&custom_count) {
        return Err(format!("custom version count={custom_count}").into());
    }
    let _ = reader.take(custom_count as usize * 20)?;
    let total_header = reader.i32()?;
    let name = reader.string()?;
    let flags = reader.u32()?;
    let name_count = reader.i32()?;
    let name_offset = reader.i32()?;
    let gatherable_count = reader.i32()?;
    let gatherable_offset = reader.i32()?;
    let export_count = reader.i32()?;
    let export_offset = reader.i32()?;
    let import_count = reader.i32()?;
    let import_offset = reader.i32()?;
    println!(
        "tag={tag:X} legacy={legacy} ue3={ue3} ue4={ue4} licensee={licensee} header={total_header} name={name:?} flags={flags:X} names=({name_count},{name_offset}) gatherable=({gatherable_count},{gatherable_offset}) exports=({export_count},{export_offset}) imports=({import_count},{import_offset})"
    );
    reader.pos = name_offset as usize;
    let mut names = Vec::new();
    for _ in 0..name_count {
        let value = reader.string()?;
        let hash = reader.u32()?;
        names.push((value, hash));
    }
    println!("names={names:?}");
    reader.pos = import_offset as usize;
    for _ in 0..import_count {
        let class_package = reader.fname()?;
        let class_name = reader.fname()?;
        let outer = reader.i32()?;
        let object_name = reader.fname()?;
        println!(
            "import class={class_package:?}/{class_name:?} outer={outer} name={object_name:?}"
        );
    }
    reader.pos = export_offset as usize;
    for _ in 0..export_count {
        let class = reader.i32()?;
        let super_index = reader.i32()?;
        let template = reader.i32()?;
        let outer = reader.i32()?;
        let object_name = reader.fname()?;
        let object_flags = reader.u32()?;
        let serial_size = reader.i64()?;
        let serial_offset = reader.i64()?;
        let forced = reader.i32()?;
        let not_client = reader.i32()?;
        let not_server = reader.i32()?;
        let _package_guid = reader.take(16)?;
        let package_flags = reader.u32()?;
        let not_always = reader.i32()?;
        let is_asset = reader.i32()?;
        let dependencies = [
            reader.i32()?,
            reader.i32()?,
            reader.i32()?,
            reader.i32()?,
            reader.i32()?,
        ];
        println!(
            "export class={class} super={super_index} template={template} outer={outer} name={object_name:?} flags={object_flags:X} size={serial_size} offset={serial_offset} forced={forced} notclient={not_client} notserver={not_server} pkgflags={package_flags:X} notalways={not_always} isasset={is_asset} deps={dependencies:?}"
        );
    }
    println!(
        "uexp_len={} first={:02X?}",
        uexp.len(),
        &uexp[..16.min(uexp.len())]
    );
    Ok(())
}
