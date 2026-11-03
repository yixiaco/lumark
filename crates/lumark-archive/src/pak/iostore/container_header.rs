//! IoStore `ContainerHeader`（chunk 类型 6）解析。
//!
//! 参考 CUE4Parse `UE4/IO/Objects/FIoContainerHeader.cs` /
//! `FFilePackageStoreEntry.cs`（Apache-2.0）的字段布局，独立实现。
//! 这里只读取包 ID 与 store entry 的导入包 ID，用于按包名/包 ID 解析
//! 跨包导入；名称表、重定向与软引用暂不展开。

use crate::error::ArchiveError;

/// UE5 ContainerHeader 签名 `IoGn`。
const SIGNATURE: u32 = 0x496f_436e;

/// 容器头。
#[derive(Debug, Clone)]
pub struct IoContainerHeader {
    /// 容器 ID。
    pub container_id: u64,
    /// 容器头版本（-2 EarlyAccess，-1 pre-version，0..=5）。
    pub version: i32,
    /// 包及其 store entry。
    pub packages: Vec<IoContainerPackage>,
}

/// 一个包的 store entry。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IoContainerPackage {
    /// 包 ID（小写包名 CityHash64）。
    pub package_id: u64,
    /// 该包直接导入的包 ID。
    pub imported_packages: Vec<u64>,
}

impl IoContainerHeader {
    /// 解析容器头字节。
    pub fn parse(data: &[u8]) -> Result<Self, ArchiveError> {
        let mut reader = Reader::new(data);
        let mut version = -1i32;
        if reader.remaining() >= 8 && reader.peek_u32()? == SIGNATURE {
            reader.skip(4)?;
            version = reader.i32()?;
        }
        let container_id = reader.u64()?;
        let packages = reader.packages(version)?;
        Ok(Self {
            container_id,
            version,
            packages,
        })
    }

    /// 按包 ID 查 store entry。
    pub fn package(&self, package_id: u64) -> Option<&IoContainerPackage> {
        self.packages
            .iter()
            .find(|package| package.package_id == package_id)
    }
}

struct Reader<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, position: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.position)
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], ArchiveError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| corrupt("ContainerHeader 偏移溢出"))?;
        let bytes = self
            .data
            .get(self.position..end)
            .ok_or_else(|| corrupt("ContainerHeader 数据越界"))?;
        self.position = end;
        Ok(bytes)
    }

    fn skip(&mut self, count: usize) -> Result<(), ArchiveError> {
        self.take(count).map(|_| ())
    }

    fn u32(&mut self) -> Result<u32, ArchiveError> {
        Ok(u32::from_le_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| corrupt("u32 长度错误"))?,
        ))
    }

    fn i32(&mut self) -> Result<i32, ArchiveError> {
        Ok(i32::from_le_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| corrupt("i32 长度错误"))?,
        ))
    }

    fn u64(&mut self) -> Result<u64, ArchiveError> {
        Ok(u64::from_le_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| corrupt("u64 长度错误"))?,
        ))
    }

    fn peek_u32(&self) -> Result<u32, ArchiveError> {
        let bytes = self
            .data
            .get(self.position..self.position + 4)
            .ok_or_else(|| corrupt("ContainerHeader 预览越界"))?;
        Ok(u32::from_le_bytes(bytes.try_into().unwrap_or([0; 4])))
    }

    fn packages(&mut self, version: i32) -> Result<Vec<IoContainerPackage>, ArchiveError> {
        if version < 2 {
            // 旧布局在 package ID 数组前有一个包数量字段。
            let _package_count = self.u32()?;
        }
        let package_ids = self.u64_array()?;
        let store_size = self.i32()?;
        if store_size < 0 {
            return Err(corrupt("ContainerHeader store entry 大小非法"));
        }
        let store_end = self
            .position
            .checked_add(store_size as usize)
            .ok_or_else(|| corrupt("ContainerHeader store entry 偏移溢出"))?;
        if store_end > self.data.len() {
            return Err(corrupt("ContainerHeader store entry 越界"));
        }
        let mut packages = Vec::with_capacity(package_ids.len());
        for package_id in package_ids {
            let imported_packages = self.store_entry(version)?;
            packages.push(IoContainerPackage {
                package_id,
                imported_packages,
            });
        }
        self.position = store_end;
        Ok(packages)
    }

    fn store_entry(&mut self, version: i32) -> Result<Vec<u64>, ArchiveError> {
        if version == -2 {
            // EarlyAccess：ExportBundlesSize + ExportCount/BundleCount + LoadOrder/Pad。
            self.skip(8)?;
            let _export_count = self.i32()?;
            let _export_bundle_count = self.i32()?;
            self.skip(8)?;
            let imported = self.u64_carray()?;
            self.skip(8)?;
            return Ok(imported);
        }
        if version >= 0 {
            if version < 3 {
                let _export_count = self.i32()?;
                let _export_bundle_count = self.i32()?;
            }
            let imported = self.u64_carray()?;
            self.skip(8)?;
            return Ok(imported);
        }
        // pre-version 或自定义布局。
        self.skip(8)?;
        let _export_count = self.i32()?;
        let _export_bundle_count = self.i32()?;
        self.skip(8)?;
        self.u64_carray()
    }

    fn u64_array(&mut self) -> Result<Vec<u64>, ArchiveError> {
        let count = self.i32()?;
        if count < 0 {
            return Err(corrupt("ContainerHeader 包数量非法"));
        }
        (0..count).map(|_| self.u64()).collect()
    }

    fn u64_carray(&mut self) -> Result<Vec<u64>, ArchiveError> {
        let initial_position = self.position;
        let count = self.i32()?;
        let offset = self.i32()?;
        if count <= 0 {
            return Ok(Vec::new());
        }
        if offset < 0 {
            return Err(corrupt("ContainerHeader CArrayView 偏移非法"));
        }
        let continue_position = self.position;
        let data_position = initial_position
            .checked_add(offset as usize)
            .ok_or_else(|| corrupt("ContainerHeader CArrayView 偏移溢出"))?;
        if data_position > self.data.len() {
            return Err(corrupt("ContainerHeader CArrayView 越界"));
        }
        self.position = data_position;
        let values = (0..count).map(|_| self.u64()).collect();
        self.position = continue_position;
        values
    }
}

fn corrupt(message: &str) -> ArchiveError {
    ArchiveError::Corrupt(message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造最小 ContainerHeader（version 3，两个包，第二个导入第一个）。
    #[test]
    fn parses_package_store_entries() {
        let mut data = Vec::new();
        data.extend_from_slice(&SIGNATURE.to_le_bytes());
        data.extend_from_slice(&3i32.to_le_bytes());
        data.extend_from_slice(&0x1122_3344u64.to_le_bytes());
        // package IDs
        data.extend_from_slice(&2i32.to_le_bytes());
        data.extend_from_slice(&10u64.to_le_bytes());
        data.extend_from_slice(&20u64.to_le_bytes());
        // store entries block
        let mut entries = Vec::new();
        // 包 1：无导入。CArrayView count=0, offset=0；随后 8 字节 ShaderMapHashes 占位。
        entries.extend_from_slice(&0i32.to_le_bytes());
        entries.extend_from_slice(&0i32.to_le_bytes());
        entries.extend_from_slice(&[0u8; 8]);
        // 包 2：导入包 10。CArrayView 数据放在当前位置之后。
        entries.extend_from_slice(&1i32.to_le_bytes());
        let offset: i32 = 16;
        entries.extend_from_slice(&offset.to_le_bytes());
        entries.extend_from_slice(&[0u8; 8]);
        entries.extend_from_slice(&10u64.to_le_bytes());
        data.extend_from_slice(&(entries.len() as i32).to_le_bytes());
        data.extend_from_slice(&entries);
        let header = IoContainerHeader::parse(&data).expect("解析");
        assert_eq!(header.version, 3);
        assert_eq!(header.container_id, 0x1122_3344);
        assert_eq!(header.packages.len(), 2);
        assert!(header.packages[0].imported_packages.is_empty());
        assert_eq!(header.packages[1].imported_packages, vec![10]);
        assert_eq!(header.package(20).unwrap().imported_packages, vec![10]);
    }
}
