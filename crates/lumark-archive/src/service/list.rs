use super::*;

impl ArchiveService {
    pub(crate) fn list_zip<R: Read + Seek>(reader: R) -> Result<ArchiveInfo, ArchiveError> {
        let mut archive = ZipArchive::new(reader)?;
        let mut info = ArchiveInfo {
            format: Some(ArchiveFormat::Zip),
            comment: Some(String::from_utf8_lossy(archive.comment()).into_owned()),
            ..Default::default()
        };
        for index in 0..archive.len() {
            let entry = archive.by_index_raw(index)?;
            let name = entry
                .name()
                .map_err(|error| ArchiveError::Corrupt(error.to_string()))?
                .to_string();
            let size = entry.size();
            let compressed = entry.compressed_size();
            let modified = zip_time_to_chrono(entry.last_modified());
            info.entries.push(ArchiveEntry {
                id: index as u64,
                path: name,
                is_dir: entry.is_dir(),
                size,
                compressed_size: Some(compressed),
                modified,
                crc32: Some(entry.crc32()),
                method: format!("{:?}", entry.compression()),
                encrypted: entry.encrypted(),
            });
            info.total_size += size;
            info.compressed_size += compressed;
            info.encrypted |= entry.encrypted();
        }
        Ok(info)
    }

    pub(crate) fn list_rar(
        path: &Path,
        password: Option<&str>,
    ) -> Result<ArchiveInfo, ArchiveError> {
        let archive = match password.filter(|value| !value.is_empty()) {
            Some(password) => UnrarArchive::with_password(path, password),
            None => UnrarArchive::new(path),
        };
        let listing = archive.open_for_listing().map_err(map_rar_error)?;
        let mut info = ArchiveInfo {
            format: Some(ArchiveFormat::Rar),
            ..Default::default()
        };
        for (index, header) in listing.enumerate() {
            let header = header.map_err(map_rar_error)?;
            let path = header.filename.to_string_lossy().replace('\\', "/");
            info.entries.push(ArchiveEntry {
                id: index as u64,
                path,
                is_dir: header.is_directory(),
                size: header.unpacked_size,
                compressed_size: None,
                modified: None,
                crc32: Some(header.file_crc),
                method: "RAR".to_string(),
                encrypted: header.is_encrypted(),
            });
            info.total_size += header.unpacked_size;
            info.encrypted |= header.is_encrypted();
        }
        Ok(info)
    }

    pub(crate) fn list_legacy(
        path: &Path,
        format: ArchiveFormat,
    ) -> Result<ArchiveInfo, ArchiveError> {
        Self::list_legacy_with(open_legacy_reader(path, format)?, format)
    }

    pub(crate) fn list_legacy_with(
        mut reader: Box<dyn LegacyReader>,
        format: ArchiveFormat,
    ) -> Result<ArchiveInfo, ArchiveError> {
        let entries = reader
            .entries()
            .map_err(|error| ArchiveError::Corrupt(error.to_string()))?;
        let mut info = ArchiveInfo {
            format: Some(format),
            ..Default::default()
        };
        for (index, entry) in entries.into_iter().enumerate() {
            let modified = entry
                .modified
                .and_then(|seconds| DateTime::<Utc>::from_timestamp(seconds as i64, 0));
            info.entries.push(ArchiveEntry {
                id: index as u64,
                path: entry.path,
                is_dir: entry.is_dir,
                size: entry.size,
                compressed_size: (entry.compressed_size > 0).then_some(entry.compressed_size),
                modified,
                crc32: entry.crc32,
                method: format.label().to_string(),
                encrypted: false,
            });
            info.total_size += entry.size;
            info.compressed_size += entry.compressed_size;
        }
        Ok(info)
    }

    pub(crate) fn list_seven_zip<R: Read + Seek>(
        reader: R,
        password: &str,
        header_encrypted: bool,
    ) -> Result<ArchiveInfo, ArchiveError> {
        let reader = ArchiveReader::new(reader, Password::from(password))
            .map_err(|error| ArchiveError::SevenZ(error.to_string()))?;
        let archive = reader.archive();
        let entries_encrypted = archive
            .blocks
            .iter()
            .flat_map(|block| block.coders.iter())
            .any(|coder| {
                coder.encoder_method_id() == sevenz_rust2::EncoderMethod::ID_AES256_SHA256
            });
        let mut info = ArchiveInfo {
            format: Some(ArchiveFormat::SevenZip),
            header_encrypted,
            encrypted: header_encrypted || entries_encrypted,
            solid: archive.is_solid,
            ..Default::default()
        };
        for (index, file) in archive.files.iter().enumerate() {
            let modified: DateTime<Utc> =
                Into::<std::time::SystemTime>::into(file.last_modified_date()).into();
            let size = file.size();
            info.entries.push(ArchiveEntry {
                id: index as u64,
                path: file.name().to_string(),
                is_dir: file.is_directory(),
                size,
                compressed_size: None,
                modified: Some(modified),
                crc32: Some(file.crc as u32),
                method: "7z".to_string(),
                encrypted: entries_encrypted,
            });
            info.total_size += size;
        }
        Ok(info)
    }

    pub(crate) fn list_tar(
        reader: Box<dyn Read>,
        format: ArchiveFormat,
    ) -> Result<ArchiveInfo, ArchiveError> {
        let mut info = ArchiveInfo {
            format: Some(format),
            ..Default::default()
        };
        let mut reader = tar_reader(reader, format)?;
        for (index, entry) in reader.entries()?.enumerate() {
            let entry = entry?;
            let path = entry.path()?.to_string_lossy().replace('\\', "/");
            let size = entry.header().size()?;
            let is_dir = entry.header().entry_type().is_dir();
            let modified = entry
                .header()
                .mtime()
                .ok()
                .and_then(|seconds| DateTime::<Utc>::from_timestamp(seconds as i64, 0));
            info.entries.push(ArchiveEntry {
                id: index as u64,
                path,
                is_dir,
                size,
                compressed_size: None,
                modified,
                crc32: None,
                method: "TAR".to_string(),
                encrypted: false,
            });
            info.total_size += size;
        }
        Ok(info)
    }

    pub(crate) fn list_single_stream(
        path: &Path,
        format: ArchiveFormat,
    ) -> Result<ArchiveInfo, ArchiveError> {
        let size = fs::metadata(path)?.len();
        let name = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("data")
            .to_string();
        Ok(Self::list_single_stream_meta(&name, size, format))
    }

    /// 按已知的单文件流名称和压缩后大小构造列表信息。
    ///
    /// 伪装包的内层文件名来自尾部信息，不能从 `.mp4` 等面具扩展名推导，
    /// 因此列表和进度中的“文件”名称统一使用内层原文件名。
    pub(crate) fn list_single_stream_meta(
        name: &str,
        size: u64,
        format: ArchiveFormat,
    ) -> ArchiveInfo {
        ArchiveInfo {
            format: Some(format),
            entries: vec![ArchiveEntry {
                id: 0,
                path: name.to_string(),
                is_dir: false,
                size: 0,
                compressed_size: Some(size),
                modified: None,
                crc32: None,
                method: format.label().to_string(),
                encrypted: false,
            }],
            total_size: 0,
            compressed_size: size,
            ..Default::default()
        }
    }

    pub(crate) fn list_unix_compress(path: &Path) -> Result<ArchiveInfo, ArchiveError> {
        let (_, entry) = open_unix_compress(path)?;
        let name = unix_compress_name(path, entry.name());
        let size = entry.original_size();
        Ok(ArchiveInfo {
            format: Some(ArchiveFormat::UnixCompress),
            entries: vec![ArchiveEntry {
                id: 0,
                path: name,
                is_dir: false,
                size,
                compressed_size: Some(entry.compressed_size()),
                modified: None,
                crc32: Some(entry.crc() as u32),
                method: entry.compression_method().to_string(),
                encrypted: false,
            }],
            total_size: size,
            compressed_size: entry.compressed_size(),
            ..Default::default()
        })
    }

    pub(crate) fn list_encrypted_container(
        path: &Path,
        password: Option<&str>,
    ) -> Result<ArchiveInfo, ArchiveError> {
        let name = encrypted_container_name(path);
        let compressed_size = fs::metadata(path)?.len();
        let size = match password.filter(|value| !value.is_empty()) {
            Some(password) => decrypt_encrypted_container(path, password)?.len() as u64,
            None => 0,
        };
        Ok(ArchiveInfo {
            format: Some(ArchiveFormat::Enc),
            entries: vec![ArchiveEntry {
                id: 0,
                path: name,
                is_dir: false,
                size,
                compressed_size: Some(compressed_size),
                modified: None,
                crc32: None,
                method: "AES-256-GCM-SIV".to_string(),
                encrypted: true,
            }],
            encrypted: true,
            total_size: size,
            compressed_size,
            ..Default::default()
        })
    }
}
