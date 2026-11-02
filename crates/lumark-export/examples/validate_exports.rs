//! 外部结构校验：GLB / PSK / PSA / UEMODEL / USDA。
//!
//! 用法：`validate_exports <file>...`。逐文件打印通过/失败项，任一失败返回非零。
//! GLB 使用官方 `gltf` crate 解析，其余格式按公开布局独立检查（不复用写出器内部状态）。

use std::error::Error;
use std::path::Path;

/// 校验结果。
struct Report {
    path: String,
    format: &'static str,
    details: Vec<String>,
    failures: Vec<String>,
}

impl Report {
    fn new(path: &Path, format: &'static str) -> Self {
        Self {
            path: path.display().to_string(),
            format,
            details: Vec::new(),
            failures: Vec::new(),
        }
    }

    fn pass(&mut self, detail: impl Into<String>) {
        self.details.push(detail.into());
    }

    fn fail(&mut self, detail: impl Into<String>) {
        self.failures.push(detail.into());
    }

    fn ok(&self) -> bool {
        self.failures.is_empty()
    }
}

/// 运行校验。
fn main() -> Result<(), Box<dyn Error>> {
    let files: Vec<String> = std::env::args().skip(1).collect();
    if files.is_empty() {
        return Err("缺少待校验文件".into());
    }
    let mut reports = Vec::new();
    for file in &files {
        let path = Path::new(file);
        let bytes = std::fs::read(path)?;
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let mut report = Report::new(path, format_label(&extension));
        match extension.as_str() {
            "glb" | "gltf" => validate_glb(&bytes, &mut report),
            "psk" => validate_psk(&bytes, &mut report),
            "psa" => validate_psa(&bytes, &mut report),
            "uemodel" => validate_ueformat(&bytes, &mut report),
            "usda" => validate_usda(&bytes, &mut report),
            other => report.fail(format!("未知格式：{other}")),
        }
        reports.push(report);
    }
    let mut failed = 0;
    for report in &reports {
        println!(
            "[{}] {} ({})",
            if report.ok() { "PASS" } else { "FAIL" },
            report.path,
            report.format
        );
        for detail in &report.details {
            println!("  - {detail}");
        }
        for failure in &report.failures {
            println!("  ! {failure}");
        }
        if !report.ok() {
            failed += 1;
        }
    }
    println!(
        "汇总：{} 个文件，{} 通过，{} 失败",
        reports.len(),
        reports.len() - failed,
        failed
    );
    if failed > 0 {
        return Err(format!("{failed} 个文件校验失败").into());
    }
    Ok(())
}

/// 扩展名对应格式标签。
fn format_label(extension: &str) -> &'static str {
    match extension {
        "glb" | "gltf" => "glTF 2.0",
        "psk" => "ActorX PSK",
        "psa" => "ActorX PSA",
        "uemodel" => "UEFormat UEMODEL",
        "usda" => "USDA",
        _ => "未知",
    }
}

/// GLB：官方 gltf crate 解析 + 基础结构。
fn validate_glb(bytes: &[u8], report: &mut Report) {
    let gltf = match gltf::Gltf::from_slice(bytes) {
        Ok(gltf) => gltf,
        Err(error) => {
            report.fail(format!("gltf 解析失败：{error}"));
            return;
        }
    };
    let document = &gltf.document;
    report.pass(format!(
        "generator={:?}",
        document.as_json().asset.generator
    ));
    if document.meshes().next().is_none() {
        report.fail("没有 mesh");
    }
    let mut primitives = 0usize;
    for mesh in document.meshes() {
        for primitive in mesh.primitives() {
            primitives += 1;
            let reader = primitive.reader(|_| gltf.blob.as_deref());
            let Some(positions) = reader.read_positions() else {
                report.fail(format!("mesh {} 缺少 POSITION", mesh.index()));
                continue;
            };
            let count = positions.count();
            if count == 0 {
                report.fail(format!("mesh {} POSITION 为空", mesh.index()));
            }
            if reader.read_indices().is_none() {
                report.fail(format!("mesh {} 缺少索引", mesh.index()));
            }
        }
    }
    report.pass(format!(
        "meshes={} primitives={} nodes={} skins={}",
        document.meshes().count(),
        primitives,
        document.nodes().count(),
        document.skins().count()
    ));
    if document.nodes().next().is_none() {
        report.fail("没有节点");
    }
    for skin in document.skins() {
        if skin.joints().next().is_none() {
            report.fail("skin 没有 joints");
        }
    }
}

/// ActorX chunk。
struct Chunk {
    name: String,
    type_flag: i32,
    data_size: i32,
    count: i32,
    offset: usize,
}

/// 解析 ActorX chunk 列表；失败返回错误信息。
fn parse_actorx_chunks(bytes: &[u8]) -> Result<Vec<Chunk>, String> {
    let mut chunks = Vec::new();
    let mut position = 0usize;
    while position + 32 <= bytes.len() {
        let name = String::from_utf8_lossy(&bytes[position..position + 20])
            .trim_end_matches('\0')
            .to_string();
        let type_flag = i32::from_le_bytes(bytes[position + 20..position + 24].try_into().unwrap());
        let data_size = i32::from_le_bytes(bytes[position + 24..position + 28].try_into().unwrap());
        let count = i32::from_le_bytes(bytes[position + 28..position + 32].try_into().unwrap());
        if data_size < 0 || count < 0 {
            return Err(format!("chunk {name} 尺寸/数量为负"));
        }
        let body = data_size as usize * count as usize;
        if position + 32 + body > bytes.len() {
            return Err(format!("chunk {name} 越界"));
        }
        chunks.push(Chunk {
            name: name.clone(),
            type_flag,
            data_size,
            count,
            offset: position + 32,
        });
        position += 32 + body;
        if name == "SCALEKEYS" {
            break;
        }
    }
    if position != bytes.len() {
        return Err(format!("尾部有 {} 字节未解析", bytes.len() - position));
    }
    Ok(chunks)
}

/// PSK：chunk 顺序、尺寸与索引范围。
fn validate_psk(bytes: &[u8], report: &mut Report) {
    let chunks = match parse_actorx_chunks(bytes) {
        Ok(chunks) => chunks,
        Err(error) => {
            report.fail(error);
            return;
        }
    };
    let names: Vec<&str> = chunks.iter().map(|chunk| chunk.name.as_str()).collect();
    report.pass(format!("chunks={}", names.join(",")));
    let Some(first) = chunks.first() else {
        report.fail("没有 chunk");
        return;
    };
    if first.name != "ACTRHEAD" || first.type_flag != 20_220_723 {
        report.fail(format!(
            "ACTRHEAD 版本错误：{} / {}",
            first.name, first.type_flag
        ));
    }
    let find = |name: &str| chunks.iter().find(|chunk| chunk.name == name);
    let Some(points) = find("PNTS0000") else {
        report.fail("缺少 PNTS0000");
        return;
    };
    if points.data_size != 12 || points.count == 0 {
        report.fail(format!(
            "PNTS0000 尺寸/数量错误：{} × {}",
            points.data_size, points.count
        ));
    }
    match find("VTXW0000") {
        Some(vtx) if vtx.data_size == 16 && vtx.count == points.count => {
            report.pass(format!("顶点={} wedge={}", points.count, vtx.count));
        }
        Some(vtx) => report.fail(format!(
            "VTXW0000 与顶点数量不一致：{} vs {}",
            vtx.count, points.count
        )),
        None => report.fail("缺少 VTXW0000"),
    }
    let face = find("FACE0000").or_else(|| find("FACE3200"));
    match face {
        Some(face) if face.count > 0 => {
            let expected_size = if face.name == "FACE0000" { 12 } else { 18 };
            if face.data_size != expected_size {
                report.fail(format!("{} 尺寸错误 {}", face.name, face.data_size));
            }
            let index_size = if face.name == "FACE0000" { 2 } else { 4 };
            let mut bad = 0;
            for triangle in 0..face.count as usize {
                let start = face.offset + triangle * expected_size as usize;
                for index in 0..3 {
                    let offset = start + index * index_size;
                    let value = if index_size == 2 {
                        u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap()) as usize
                    } else {
                        u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize
                    };
                    if value >= points.count as usize {
                        bad += 1;
                    }
                }
            }
            if bad > 0 {
                report.fail(format!("{} 个顶点索引越界", bad));
            }
            report.pass(format!("三角形={}", face.count));
        }
        Some(face) => report.fail(format!("{} 数量为 0", face.name)),
        None => report.fail("缺少 FACE0000/FACE3200"),
    }
    match find("MATT0000") {
        Some(materials) if materials.data_size == 88 => {
            report.pass(format!("材质槽={}", materials.count));
        }
        Some(materials) => report.fail(format!("MATT0000 尺寸错误 {}", materials.data_size)),
        None => report.fail("缺少 MATT0000"),
    }
    if let Some(normals) = find("VTXNORMS") {
        if normals.count != points.count || normals.data_size != 12 {
            report.fail("VTXNORMS 数量/尺寸错误");
        }
    } else {
        report.fail("缺少 VTXNORMS");
    }
    let weights = find("RAWWEIGHTS");
    let bones = find("REFSKELT").or_else(|| find("REFSKEL32"));
    match (weights, bones) {
        (Some(weights), Some(bones))
            if weights.data_size == 12 && weights.count == 0 && bones.count == 0 =>
        {
            report.pass("无蒙皮（静态网格）");
        }
        (Some(weights), Some(bones)) if weights.data_size == 12 && bones.count > 0 => {
            report.pass(format!("权重={} 骨骼={}", weights.count, bones.count));
        }
        (None, _) => {
            report.pass("无蒙皮（静态网格）");
        }
        _ => report.fail("蒙皮 chunk 不完整或尺寸错误"),
    }
}

/// PSA：chunk 顺序、动画信息与关键帧数量。
fn validate_psa(bytes: &[u8], report: &mut Report) {
    let chunks = match parse_actorx_chunks(bytes) {
        Ok(chunks) => chunks,
        Err(error) => {
            report.fail(error);
            return;
        }
    };
    let names: Vec<&str> = chunks.iter().map(|chunk| chunk.name.as_str()).collect();
    report.pass(format!("chunks={}", names.join(",")));
    let expected = ["ANIMHEAD", "BONENAMES", "ANIMINFO", "ANIMKEYS", "SCALEKEYS"];
    if names != expected {
        report.fail(format!("chunk 顺序错误：{:?}", names));
    }
    let find = |name: &str| chunks.iter().find(|chunk| chunk.name == name);
    let Some(head) = chunks.first() else {
        report.fail("没有 chunk");
        return;
    };
    if head.type_flag != 20_100_422 {
        report.fail(format!("PSA 版本错误：{}", head.type_flag));
    }
    let bones = find("BONENAMES").map(|chunk| chunk.count).unwrap_or(0);
    let Some(info) = find("ANIMINFO") else {
        report.fail("缺少 ANIMINFO");
        return;
    };
    if info.data_size != 168 || info.count != 1 {
        report.fail(format!(
            "ANIMINFO 尺寸错误 {} × {}",
            info.data_size, info.count
        ));
        return;
    }
    let body = &bytes[info.offset..info.offset + info.data_size as usize];
    let total_bones = i32::from_le_bytes(body[128..132].try_into().unwrap());
    let key_quotum = i32::from_le_bytes(body[140..144].try_into().unwrap());
    let track_time = f32::from_le_bytes(body[148..152].try_into().unwrap());
    let anim_rate = f32::from_le_bytes(body[152..156].try_into().unwrap());
    let raw_frames = i32::from_le_bytes(body[164..168].try_into().unwrap());
    if total_bones != bones {
        report.fail(format!("TotalBones {total_bones} != BONENAMES {bones}"));
    }
    if raw_frames <= 0 || track_time <= 0.0 || anim_rate <= 0.0 {
        report.fail(format!(
            "动画信息异常：frames={raw_frames} time={track_time} rate={anim_rate}"
        ));
    }
    if key_quotum != raw_frames.saturating_mul(bones) {
        report.fail(format!(
            "KeyQuotum {key_quotum} != frames {raw_frames} × bones {bones}"
        ));
    }
    let Some(keys) = find("ANIMKEYS") else {
        report.fail("缺少 ANIMKEYS");
        return;
    };
    let Some(scales) = find("SCALEKEYS") else {
        report.fail("缺少 SCALEKEYS");
        return;
    };
    let expected_keys = raw_frames.saturating_mul(bones);
    if keys.count != expected_keys || scales.count != expected_keys {
        report.fail(format!(
            "关键帧数量错误：keys={} scales={} expected={expected_keys}",
            keys.count, scales.count
        ));
    }
    if keys.data_size != 32 || scales.data_size != 16 {
        report.fail(format!(
            "关键帧尺寸错误：keys={} scales={}",
            keys.data_size, scales.data_size
        ));
    }
    let mut bad_quat = 0usize;
    for index in 0..keys.count as usize {
        let start = keys.offset + index * 32 + 12;
        let quat = [
            f32::from_le_bytes(bytes[start..start + 4].try_into().unwrap()),
            f32::from_le_bytes(bytes[start + 4..start + 8].try_into().unwrap()),
            f32::from_le_bytes(bytes[start + 8..start + 12].try_into().unwrap()),
            f32::from_le_bytes(bytes[start + 12..start + 16].try_into().unwrap()),
        ];
        let norm =
            (quat[0] * quat[0] + quat[1] * quat[1] + quat[2] * quat[2] + quat[3] * quat[3]).sqrt();
        if !(0.5..1.5).contains(&norm) {
            bad_quat += 1;
        }
    }
    if bad_quat > 0 {
        report.fail(format!("{bad_quat} 个四元数未归一化"));
    }
    let mut parent_bad = 0;
    for bone in 0..bones as usize {
        let start = bytes.get(
            find("BONENAMES")
                .map(|chunk| chunk.offset + bone * 120 + 72)
                .unwrap_or(0)..,
        );
        if let Some(start) = start
            && let Some(slice) = start.get(..4)
        {
            let parent = i32::from_le_bytes(slice.try_into().unwrap());
            if !(-1..bones).contains(&parent) {
                parent_bad += 1;
            }
        }
    }
    if parent_bad > 0 {
        report.fail(format!("{parent_bad} 个骨骼父索引越界"));
    }
    report.pass(format!(
        "骨骼={bones} 帧={raw_frames} fps={anim_rate:.2} 关键帧={}",
        keys.count
    ));
}

/// UEFormat：魔数、版本、头部与根属性。
fn validate_ueformat(bytes: &[u8], report: &mut Report) {
    if !bytes.starts_with(b"UEFORMAT") {
        report.fail("缺少 UEFORMAT 魔数");
        return;
    }
    let mut position = 8usize;
    let read_string = |position: &mut usize| -> Option<String> {
        let length_bytes = bytes.get(*position..*position + 4)?;
        let length = i32::from_le_bytes(length_bytes.try_into().ok()?) as usize;
        *position += 4;
        let text = bytes.get(*position..*position + length)?;
        *position += length;
        Some(
            String::from_utf8_lossy(text)
                .trim_end_matches('\0')
                .to_string(),
        )
    };
    let Some(kind) = read_string(&mut position) else {
        report.fail("读取 UEMODEL 标识失败");
        return;
    };
    if kind != "UEMODEL" {
        report.fail(format!("标识错误：{kind}"));
    }
    let version = bytes.get(position).copied().unwrap_or(0);
    position += 1;
    if version != 10 {
        report.fail(format!("版本错误：{version}"));
    }
    let Some(name) = read_string(&mut position) else {
        report.fail("读取对象名失败");
        return;
    };
    let path = read_string(&mut position).unwrap_or_default();
    position += 1; // is_compressed
    let attribute_count = bytes
        .get(position..position + 4)
        .map(|slice| i32::from_le_bytes(slice.try_into().unwrap()))
        .unwrap_or(0);
    position += 4;
    if !(0..=16).contains(&attribute_count) {
        report.fail(format!("根属性数量异常：{attribute_count}"));
        return;
    }
    let mut names = Vec::new();
    for _ in 0..attribute_count {
        let Some(attribute_name) = read_string(&mut position) else {
            report.fail("读取根属性名失败");
            return;
        };
        let Some(length_bytes) = bytes.get(position..position + 4) else {
            report.fail("读取根属性长度失败");
            return;
        };
        let length = i32::from_le_bytes(length_bytes.try_into().unwrap());
        position += 4;
        if length < 0 || position + length as usize > bytes.len() {
            report.fail(format!("根属性 {attribute_name} 越界"));
            return;
        }
        position += length as usize;
        names.push(attribute_name);
    }
    if position != bytes.len() {
        report.fail(format!("尾部有 {} 字节未解析", bytes.len() - position));
    }
    if !names.iter().any(|name| name == "LODS") {
        report.fail("缺少 LODS");
    }
    report.pass(format!(
        "name={name} path={path} version={version} roots={}",
        names.join(",")
    ));
    if !names.iter().any(|name| name == "SKELETON") {
        report.pass("无骨架（静态网格）");
    }
}

/// 判断文本中是否出现独立的 NaN/Inf 数值（避免误伤 `info:id` 等单词）。
fn has_non_finite_token(text: &str) -> bool {
    text.split(|character: char| {
        character.is_whitespace() || matches!(character, ',' | '(' | ')' | '[' | ']' | '{' | '}')
    })
    .any(|token| {
        matches!(
            token.trim_matches('"'),
            "nan" | "-nan" | "NaN" | "inf" | "-inf" | "Infinity" | "-Infinity"
        )
    })
}

/// USDA：文本头、单位、结构与数值合法性。
fn validate_usda(bytes: &[u8], report: &mut Report) {
    let Ok(text) = std::str::from_utf8(bytes) else {
        report.fail("不是 UTF-8 文本");
        return;
    };
    if !text.starts_with("#usda 1.0") {
        report.fail("缺少 #usda 1.0 头");
    }
    for required in ["metersPerUnit", "def Mesh", "def Material"] {
        if !text.contains(required) {
            report.fail(format!("缺少 {required}"));
        }
    }
    if !text.contains("def Xform \"Root\"") && !text.contains("def SkelRoot \"Root\"") {
        report.fail("缺少 def Xform/SkelRoot \"Root\"");
    }
    if text.contains("def Skeleton") {
        for required in [
            "uniform token[] joints =",
            "matrix4d[] bindTransforms =",
            "matrix4d[] restTransforms =",
            "rel skel:skeleton =",
            "primvars:skel:jointIndices",
            "primvars:skel:jointWeights",
        ] {
            if !text.contains(required) {
                report.fail(format!("骨骼资产缺少 {required}"));
            }
        }
    }
    if has_non_finite_token(text) {
        report.fail("包含 NaN/Inf 数值");
    }
    let open = text.chars().filter(|character| *character == '{').count();
    let close = text.chars().filter(|character| *character == '}').count();
    if open != close {
        report.fail(format!("花括号不匹配：{open} / {close}"));
    }
    if text.chars().filter(|character| *character == '"').count() % 2 != 0 {
        report.fail("引号数量为奇数");
    }
    report.pass(format!(
        "字节={} 行={} 花括号={open}",
        text.len(),
        text.lines().count()
    ));
}
