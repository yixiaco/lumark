//! 手工诊断：打印 AnimSequence 的属性名/字节布局，用于确认动画压缩格式。
//!
//! 用法：`anim_probe <game-root> <mapping.usmap> <oodle-dll> <container> <entry>`

use std::error::Error;
use std::path::Path;

use lumark_archive::{IoStoreContainer, configure_oodle_library};
use lumark_unreal::{Mappings, ZenScriptObjects, extract_key_for_game, parse_package, scan_game};

/// 运行诊断。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("缺少游戏目录")?;
    let mapping_path = args.next().ok_or("缺少 .usmap")?;
    let oodle = args.next().ok_or("缺少 Oodle DLL")?;
    let container_name = args.next().ok_or("缺少容器文件名")?;
    let entry = args.next().ok_or("缺少资产路径")?;
    configure_oodle_library(Some(oodle.into())).map_err(|error| format!("Oodle：{error}"))?;
    let game = scan_game(Path::new(&root))?;
    let container_path = game
        .containers
        .iter()
        .find(|container| {
            container
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == container_name)
        })
        .map(|container| container.path.clone())
        .ok_or("未找到指定容器")?;
    let key = extract_key_for_game(&game, None, |_, _, _| {})
        .key
        .ok_or("未提取到 AES 密钥")?;
    let container = IoStoreContainer::open(&container_path, Some(key.as_str()))?;
    let data = container.read_file(&entry)?;
    let global = container_path
        .parent()
        .map(|parent| parent.join("global.utoc"))
        .filter(|path| path.is_file());
    let scripts = global
        .as_deref()
        .map(|path| ZenScriptObjects::from_container(path, Some(key.as_str())))
        .transpose()?;
    let mappings = Mappings::parse_file(Path::new(&mapping_path))?;
    let package = parse_package(data, scripts.as_ref(), None)?;
    for (index, export) in package.info().exports.iter().enumerate() {
        println!("export {index}: {} ({})", export.name, export.class_name);
    }
    let index = package
        .info()
        .exports
        .iter()
        .position(|export| export.class_name == "AnimSequence")
        .ok_or("未找到 AnimSequence 导出")?;
    let properties = package.read_export_properties(index, &mappings)?;
    println!(
        "export={} props={} bytes_read={}",
        package.info().exports[index].name,
        properties.properties.len(),
        properties.bytes_read
    );
    for (name, value) in &properties.properties {
        println!("  {name} = {value:?}");
    }
    if let Some(export_data) = package.export_data(index) {
        let rest = export_data.get(properties.bytes_read..).unwrap_or_default();
        println!("export_data={} rest={}", export_data.len(), rest.len());
        if let Some((start, tracks)) = find_animation_layout(rest) {
            println!("layout_start={start} tracks={tracks}");
            dump_track_formats(rest, start, tracks);
        }
        let preview: Vec<String> = rest
            .iter()
            .take(64)
            .map(|byte| format!("{byte:02X}"))
            .collect();
        println!("rest bytes: {}", preview.join(" "));
    }
    Ok(())
}

/// 在 rest 数据中定位 `CompressedRawDataSize/TrackToSkeletonMap`。
fn find_animation_layout(data: &[u8]) -> Option<(usize, usize)> {
    for offset in 0..data.len().saturating_sub(16) {
        let Some(raw_bytes) = data.get(offset..offset + 4) else {
            continue;
        };
        let Some(track_bytes) = data.get(offset + 4..offset + 8) else {
            continue;
        };
        let raw_size = i32::from_le_bytes(raw_bytes.try_into().ok()?);
        let tracks = i32::from_le_bytes(track_bytes.try_into().ok()?);
        if !(0..=100_000_000).contains(&raw_size) || !(1..=65535).contains(&tracks) {
            continue;
        }
        let tracks = tracks as usize;
        let map_end = offset + 8 + tracks * 4;
        let Some(curve_bytes) = data.get(map_end..map_end + 4) else {
            continue;
        };
        let curves = i32::from_le_bytes(curve_bytes.try_into().ok()?);
        if curves == 0 {
            return Some((offset, tracks));
        }
    }
    None
}

/// 打印每条 track 的 trans/rot header 格式。
fn dump_track_formats(data: &[u8], start: usize, tracks: usize) {
    let stream_bytes_offset = start + 8 + tracks * 4 + 4 + 4 + 4; // raw,map,curves,num,bool
    let num_bytes = i32::from_le_bytes(
        data.get(stream_bytes_offset - 8..stream_bytes_offset - 4)
            .unwrap_or_default()
            .try_into()
            .unwrap_or_default(),
    );
    println!("stream_bytes={num_bytes}");
    let stream_start = stream_bytes_offset;
    let stream_end = stream_start.saturating_add(num_bytes.max(0) as usize);
    let Some(stream) = data.get(stream_start..stream_end.min(data.len())) else {
        return;
    };
    let Some(offset_count) = stream
        .get(..4)
        .and_then(|bytes| bytes.try_into().ok())
        .map(i32::from_le_bytes)
    else {
        return;
    };
    println!("offset_count={offset_count}");
    if offset_count < (tracks * 2) as i32 {
        return;
    }
    let mut formats = std::collections::BTreeMap::new();
    for track in 0..tracks {
        for slot in 0..2 {
            let offset_pos = 4 + (track * 2 + slot) * 4;
            let Some(offset) = stream
                .get(offset_pos..offset_pos + 4)
                .and_then(|bytes| bytes.try_into().ok())
                .map(i32::from_le_bytes)
            else {
                return;
            };
            if offset < 0 {
                continue;
            }
            let header_pos = 4 + offset as usize;
            let Some(header) = stream
                .get(header_pos..header_pos + 4)
                .and_then(|bytes| bytes.try_into().ok())
                .map(i32::from_le_bytes)
            else {
                continue;
            };
            let key_format = (header >> 28) & 0xF;
            let format_flags = (header >> 24) & 0xF;
            let num_keys = header & 0x00FF_FFFF;
            *formats.entry((key_format, format_flags)).or_insert(0usize) += 1;
            if track < 5 {
                println!(
                    "track={track} slot={slot} offset={offset} keys={num_keys} format={key_format} flags={format_flags}"
                );
            }
        }
    }
    println!("formats={formats:?}");
}
