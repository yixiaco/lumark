//! 用 `gltf` crate 校验生成的 GLB 结构。
//!
//! 用法：`validate_glb <file.glb>`

use std::error::Error;

/// 运行校验。
fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args().nth(1).ok_or("缺少 .glb 路径")?;
    let data = std::fs::read(&path)?;
    let gltf = gltf::Gltf::from_slice(&data)?;
    let document = &gltf.document;
    let primitives: usize = document
        .meshes()
        .map(|mesh| mesh.primitives().count())
        .sum();
    println!(
        "glb={path} meshes={} primitives={} nodes={} accessors={} materials={} images={} buffers={}",
        document.meshes().count(),
        primitives,
        document.nodes().count(),
        document.accessors().count(),
        document.materials().count(),
        document.images().count(),
        document.buffers().count(),
    );
    for mesh in document.meshes() {
        for primitive in mesh.primitives() {
            let attributes = primitive.attributes().count();
            let indices = primitive.indices().map(|accessor| accessor.count());
            println!(
                "mesh={:?} primitive_attributes={} indices={:?} material={:?}",
                mesh.name(),
                attributes,
                indices,
                primitive.material().name()
            );
        }
    }
    Ok(())
}
