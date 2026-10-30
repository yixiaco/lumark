//! Windows 虚拟文件拖放：拖动时只提供文件描述，投放后才解压。
//!
//! 资源管理器在投放阶段通过 `IDataObject` 的 `FileContents` 格式向本进程请求
//! 数据；这里用自定义 `IStream` 在首次 `Read` 时才调用数据源解压到临时文件，
//! 因此取消拖动不会产生临时文件，超大文件也不会在拖动开始时被完整读入内存。

use std::io::Read;
use std::mem::ManuallyDrop;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use windows::Win32::Foundation::{
    DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, DV_E_FORMATETC, DV_E_LINDEX,
    DV_E_TYMED, E_FAIL, E_NOTIMPL, FILETIME, HGLOBAL, OLE_E_ADVISENOTSUPPORTED, S_FALSE, S_OK,
};
use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL};
use windows::Win32::System::Com::{
    DATADIR_GET, DVASPECT_CONTENT, FORMATETC, IDataObject, IDataObject_Impl, IEnumFORMATETC,
    IEnumFORMATETC_Impl, ISequentialStream_Impl, IStream, IStream_Impl, LOCKTYPE, STATFLAG,
    STATSTG, STGC, STGM, STGMEDIUM, STGMEDIUM_0, STGTY_STREAM, STREAM_SEEK, STREAM_SEEK_CUR,
    STREAM_SEEK_END, STREAM_SEEK_SET, TYMED_HGLOBAL, TYMED_ISTREAM,
};
use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::{
    CF_HDROP, DROPEFFECT, DROPEFFECT_COPY, IDropSource, IDropSource_Impl,
};
use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
use windows::Win32::UI::Shell::{
    CFSTR_FILECONTENTS, CFSTR_FILEDESCRIPTORW, CFSTR_PREFERREDDROPEFFECT, FD_ATTRIBUTES,
    FD_FILESIZE, FD_WRITESTIME, FILEDESCRIPTORW, SHDoDragDrop,
};
use windows::core::{BOOL, HRESULT, Ref, Result as WinResult, implement};

use crate::PlatformError;

/// 拖出的虚拟文件描述；名称使用归档内相对路径（反斜杠分隔）。
#[derive(Debug, Clone)]
pub struct VirtualFile {
    /// 相对名称，可包含子目录。
    pub name: String,
    /// 原始大小。
    pub size: u64,
    /// 是否为目录描述符。
    pub is_dir: bool,
    /// 修改时间，用于资源管理器显示。
    pub modified: Option<std::time::SystemTime>,
}

/// 虚拟文件数据源：提供描述符，并在投放后按索引解压单个文件。
pub trait VirtualFileSource: Send + Sync {
    /// 返回拖出的文件/目录描述。
    fn files(&self) -> Vec<VirtualFile>;

    /// 把第 `index` 个文件解压到 `destination`。
    ///
    /// 只会在资源管理器真正请求文件内容时调用；目录描述符不会触发解压。
    fn extract(&self, index: usize, destination: &Path) -> Result<(), String>;
}

/// 启动原生 OLE 拖放，返回是否发生了实际投放。
///
/// 该入口要求调用线程已经初始化 OLE，并拥有传入的窗口；GPUI 主线程满足这
/// 两个条件。真正的解压仍由 `IStream::Read` 在投放后触发。
pub fn start_virtual_file_drag(
    hwnd: isize,
    source: Arc<dyn VirtualFileSource>,
) -> Result<bool, PlatformError> {
    if unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } >= 0 {
        return Ok(false);
    }
    let files = source.files();
    if files.is_empty() {
        return Err(PlatformError::EmptyPaths);
    }
    let data_object: IDataObject = VirtualDataObject::new(source, files).into();
    let drop_source: IDropSource = VirtualDropSource.into();
    let effect = unsafe {
        SHDoDragDrop(
            Some(windows::Win32::Foundation::HWND(
                hwnd as *mut core::ffi::c_void,
            )),
            &data_object,
            &drop_source,
            DROPEFFECT_COPY,
        )
        .map_err(|error| PlatformError::Windows(std::io::Error::other(error.to_string())))?
    };
    Ok(effect.0 != 0)
}

/// 只携带“复制”效果的原生拖拽数据对象。
///
/// 外部拖出只用它让 Windows 显示系统拖拽光标；真正解压由 Lumark 在释放后
/// 根据目标窗口自行完成，因此不会让资源管理器读取虚拟文件流。
#[implement(IDataObject)]
struct DragGestureDataObject {
    drop_effect_format: u16,
    formats: Vec<FORMATETC>,
}

impl DragGestureDataObject {
    /// 创建只声明 `Preferred DropEffect` 的数据对象。
    fn new() -> Self {
        let drop_effect_format =
            unsafe { RegisterClipboardFormatW(CFSTR_PREFERREDDROPEFFECT) } as u16;
        Self {
            drop_effect_format,
            formats: vec![
                format_etc(drop_effect_format, TYMED_HGLOBAL.0),
                format_etc(CF_HDROP.0, TYMED_HGLOBAL.0),
            ],
        }
    }
}

/// 构造空的 `CF_HDROP` 数据，仅让资源管理器接受投放并显示复制光标。
///
/// 资源管理器复制 0 个文件；真正的文件由 Lumark 在释放后解压。
fn empty_hdrop() -> Vec<u8> {
    #[repr(C)]
    #[derive(Default)]
    struct DropFiles {
        p_files: u32,
        pt: windows::Win32::Foundation::POINT,
        f_nc: i32,
        f_wide: i32,
    }
    let header = DropFiles {
        p_files: std::mem::size_of::<DropFiles>() as u32,
        f_wide: 1,
        ..Default::default()
    };
    let header_bytes = unsafe {
        std::slice::from_raw_parts(
            (&header as *const DropFiles).cast::<u8>(),
            std::mem::size_of::<DropFiles>(),
        )
    };
    let mut bytes = header_bytes.to_vec();
    bytes.extend_from_slice(&[0u8; 4]);
    bytes
}

impl IDataObject_Impl for DragGestureDataObject_Impl {
    /// 只返回复制效果；不暴露文件内容。
    fn GetData(&self, pformatetcin: *const FORMATETC) -> WinResult<STGMEDIUM> {
        let format = unsafe { &*pformatetcin };
        if format.cfFormat == self.drop_effect_format && format.tymed & TYMED_HGLOBAL.0 as u32 != 0
        {
            let bytes = DROPEFFECT_COPY.0.to_le_bytes();
            return Ok(STGMEDIUM {
                tymed: TYMED_HGLOBAL.0 as u32,
                u: STGMEDIUM_0 {
                    hGlobal: global_from_bytes(&bytes)?,
                },
                pUnkForRelease: ManuallyDrop::new(None),
            });
        }
        if format.cfFormat == CF_HDROP.0 && format.tymed & TYMED_HGLOBAL.0 as u32 != 0 {
            return Ok(STGMEDIUM {
                tymed: TYMED_HGLOBAL.0 as u32,
                u: STGMEDIUM_0 {
                    hGlobal: global_from_bytes(&empty_hdrop())?,
                },
                pUnkForRelease: ManuallyDrop::new(None),
            });
        }
        Err(hresult_error(DV_E_FORMATETC))
    }

    /// 不支持由目标提供存储介质。
    fn GetDataHere(
        &self,
        _pformatetc: *const FORMATETC,
        _pmedium: *mut STGMEDIUM,
    ) -> WinResult<()> {
        Err(hresult_error(E_NOTIMPL))
    }

    /// 只支持首选拖放效果。
    fn QueryGetData(&self, pformatetc: *const FORMATETC) -> HRESULT {
        let format = unsafe { &*pformatetc };
        if (format.cfFormat == self.drop_effect_format || format.cfFormat == CF_HDROP.0)
            && format.tymed & TYMED_HGLOBAL.0 as u32 != 0
        {
            S_OK
        } else {
            DV_E_FORMATETC
        }
    }

    /// 不支持格式转换。
    fn GetCanonicalFormatEtc(
        &self,
        _pformatectin: *const FORMATETC,
        _pformatetcout: *mut FORMATETC,
    ) -> HRESULT {
        E_NOTIMPL
    }

    /// 拖拽数据对象只读。
    fn SetData(
        &self,
        _pformatetc: *const FORMATETC,
        _pmedium: *const STGMEDIUM,
        _frelease: BOOL,
    ) -> WinResult<()> {
        Err(hresult_error(E_NOTIMPL))
    }

    /// 枚举首选拖放效果。
    fn EnumFormatEtc(&self, dwdirection: u32) -> WinResult<IEnumFORMATETC> {
        if dwdirection != DATADIR_GET.0 as u32 {
            return Err(hresult_error(E_NOTIMPL));
        }
        Ok(VirtualFormatEnum::new(self.formats.clone()).into())
    }

    /// 不支持拖放建议。
    fn DAdvise(
        &self,
        _pformatetc: *const FORMATETC,
        _advf: u32,
        _padvsink: Ref<'_, windows::Win32::System::Com::IAdviseSink>,
    ) -> WinResult<u32> {
        Err(hresult_error(OLE_E_ADVISENOTSUPPORTED))
    }

    /// 不支持拖放建议。
    fn DUnadvise(&self, _dwconnection: u32) -> WinResult<()> {
        Err(hresult_error(OLE_E_ADVISENOTSUPPORTED))
    }

    /// 不支持拖放建议。
    fn EnumDAdvise(&self) -> WinResult<windows::Win32::System::Com::IEnumSTATDATA> {
        Err(hresult_error(OLE_E_ADVISENOTSUPPORTED))
    }
}

/// 原生拖拽手势的结果。
pub struct DragGestureOutcome {
    /// 目标是否接受了投放；目标拒绝时仍可按鼠标位置解析目录。
    pub dropped: bool,
    /// 用户是否按 Esc 取消了拖拽。
    pub cancelled: bool,
}

/// 启动一次只用于选择目标窗口的原生拖拽手势。
pub fn start_drag_gesture(hwnd: isize) -> Result<DragGestureOutcome, PlatformError> {
    if unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } >= 0 {
        return Ok(DragGestureOutcome {
            dropped: false,
            cancelled: true,
        });
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    let data_object: IDataObject = DragGestureDataObject::new().into();
    let drop_source: IDropSource = GestureDropSource {
        cancelled: Arc::clone(&cancelled),
    }
    .into();
    let effect = unsafe {
        SHDoDragDrop(
            Some(windows::Win32::Foundation::HWND(
                hwnd as *mut core::ffi::c_void,
            )),
            &data_object,
            &drop_source,
            DROPEFFECT_COPY,
        )
        .map_err(|error| PlatformError::Windows(std::io::Error::other(error.to_string())))?
    };
    Ok(DragGestureOutcome {
        dropped: effect.0 != 0,
        cancelled: cancelled.load(Ordering::Relaxed),
    })
}

/// 记录 Esc 取消状态的手势拖拽源。
#[implement(IDropSource)]
struct GestureDropSource {
    cancelled: Arc<AtomicBool>,
}

impl IDropSource_Impl for GestureDropSource_Impl {
    /// 松开左键时完成手势，按 Esc 时取消并记录状态。
    fn QueryContinueDrag(&self, fescapepressed: BOOL, grfkeystate: MODIFIERKEYS_FLAGS) -> HRESULT {
        if fescapepressed.as_bool() {
            self.cancelled.store(true, Ordering::Relaxed);
            return DRAGDROP_S_CANCEL;
        }
        if (grfkeystate.0 & MK_LBUTTON.0) == 0 {
            return DRAGDROP_S_DROP;
        }
        S_OK
    }

    /// 使用系统拖放光标；`CF_HDROP` 让资源管理器显示复制光标。
    fn GiveFeedback(&self, _dweffect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

/// 构造一个空内容的文件组描述符格式。
fn format_etc(cf_format: u16, tymed: i32) -> FORMATETC {
    FORMATETC {
        cfFormat: cf_format,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: tymed as u32,
    }
}

/// 把 HRESULT 转换为可返回的 COM 错误。
fn hresult_error(code: HRESULT) -> windows::core::Error {
    windows::core::Error::from_hresult(code)
}

/// 用 HGLOBAL 分配并复制字节，供 OLE 结构体返回给 Shell。
fn global_from_bytes(bytes: &[u8]) -> WinResult<HGLOBAL> {
    let memory = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len())? };
    let pointer = unsafe { GlobalLock(memory) } as *mut u8;
    if pointer.is_null() {
        unsafe {
            let _ = windows::Win32::Foundation::GlobalFree(Some(memory));
        }
        return Err(hresult_error(E_FAIL));
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer, bytes.len());
        let _ = GlobalUnlock(memory);
    }
    Ok(memory)
}

/// 把系统时间转换为 FILETIME；早于 1970 年的时间按零处理。
fn file_time(time: std::time::SystemTime) -> FILETIME {
    let duration = time
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let ticks = (duration.as_secs() + 11_644_473_600)
        .saturating_mul(10_000_000)
        .saturating_add(u64::from(duration.subsec_nanos()) / 100);
    FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    }
}

/// 构造单个 `FILEDESCRIPTORW`，名称过长时返回失败而不是截断。
fn file_descriptor(file: &VirtualFile) -> WinResult<FILEDESCRIPTORW> {
    let name = file.name.replace('/', "\\");
    let utf16: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    if utf16.len() > 260 {
        return Err(hresult_error(E_FAIL));
    }
    let mut name_buffer = [0u16; 260];
    name_buffer[..utf16.len()].copy_from_slice(&utf16);
    let descriptor = FILEDESCRIPTORW {
        dwFlags: (FD_ATTRIBUTES.0 | FD_FILESIZE.0 | FD_WRITESTIME.0) as u32,
        dwFileAttributes: if file.is_dir {
            FILE_ATTRIBUTE_DIRECTORY.0
        } else {
            FILE_ATTRIBUTE_NORMAL.0
        },
        nFileSizeHigh: (file.size >> 32) as u32,
        nFileSizeLow: file.size as u32,
        ftLastWriteTime: file.modified.map(file_time).unwrap_or_default(),
        cFileName: name_buffer,
        ..Default::default()
    };
    Ok(descriptor)
}

/// 文件组描述符数据对象。
#[implement(IDataObject)]
struct VirtualDataObject {
    source: Arc<dyn VirtualFileSource>,
    files: Vec<VirtualFile>,
    formats: Vec<FORMATETC>,
    descriptor_format: u16,
    contents_format: u16,
    drop_effect_format: u16,
}

impl VirtualDataObject {
    /// 为给定文件列表构造数据对象。
    fn new(source: Arc<dyn VirtualFileSource>, files: Vec<VirtualFile>) -> Self {
        let descriptor_format = unsafe { RegisterClipboardFormatW(CFSTR_FILEDESCRIPTORW) } as u16;
        let contents_format = unsafe { RegisterClipboardFormatW(CFSTR_FILECONTENTS) } as u16;
        let drop_effect_format =
            unsafe { RegisterClipboardFormatW(CFSTR_PREFERREDDROPEFFECT) } as u16;
        let formats = vec![
            format_etc(descriptor_format, TYMED_HGLOBAL.0),
            format_etc(contents_format, TYMED_ISTREAM.0),
            format_etc(drop_effect_format, TYMED_HGLOBAL.0),
        ];
        Self {
            source,
            files,
            formats,
            descriptor_format,
            contents_format,
            drop_effect_format,
        }
    }

    /// 构建 `FILEGROUPDESCRIPTORW` 的 HGLOBAL 数据。
    fn file_group(&self) -> WinResult<HGLOBAL> {
        let item_size = std::mem::size_of::<FILEDESCRIPTORW>();
        let total = std::mem::size_of::<u32>() + self.files.len() * item_size;
        let memory = unsafe { GlobalAlloc(GMEM_MOVEABLE, total)? };
        let pointer = unsafe { GlobalLock(memory) } as *mut u8;
        if pointer.is_null() {
            unsafe {
                let _ = windows::Win32::Foundation::GlobalFree(Some(memory));
            }
            return Err(hresult_error(E_FAIL));
        }
        unsafe {
            std::ptr::write_bytes(pointer, 0, total);
            std::ptr::copy_nonoverlapping(
                (self.files.len() as u32).to_le_bytes().as_ptr(),
                pointer,
                std::mem::size_of::<u32>(),
            );
        }
        for (index, file) in self.files.iter().enumerate() {
            let descriptor = file_descriptor(file)?;
            let slot = unsafe {
                pointer
                    .add(std::mem::size_of::<u32>() + index * item_size)
                    .cast::<FILEDESCRIPTORW>()
            };
            unsafe { slot.write_unaligned(descriptor) };
        }
        unsafe {
            let _ = GlobalUnlock(memory);
        }
        Ok(memory)
    }

    /// 构造指定索引的延迟解压流。
    fn contents_stream(&self, index: usize) -> WinResult<IStream> {
        if index >= self.files.len() || self.files[index].is_dir {
            return Err(hresult_error(DV_E_LINDEX));
        }
        Ok(VirtualStream::new(self.source.clone(), index, self.files[index].size).into())
    }
}

impl IDataObject_Impl for VirtualDataObject_Impl {
    /// 按格式返回文件组描述符、单个文件内容或首选拖放效果。
    fn GetData(&self, pformatetcin: *const FORMATETC) -> WinResult<STGMEDIUM> {
        let format = unsafe { &*pformatetcin };
        if format.cfFormat == self.descriptor_format {
            if format.tymed & TYMED_HGLOBAL.0 as u32 == 0 {
                return Err(hresult_error(DV_E_TYMED));
            }
            return Ok(STGMEDIUM {
                tymed: TYMED_HGLOBAL.0 as u32,
                u: STGMEDIUM_0 {
                    hGlobal: self.file_group()?,
                },
                pUnkForRelease: ManuallyDrop::new(None),
            });
        }
        if format.cfFormat == self.contents_format {
            if format.tymed & TYMED_ISTREAM.0 as u32 == 0 {
                return Err(hresult_error(DV_E_TYMED));
            }
            if format.lindex < 0 {
                return Err(hresult_error(DV_E_LINDEX));
            }
            return Ok(STGMEDIUM {
                tymed: TYMED_ISTREAM.0 as u32,
                u: STGMEDIUM_0 {
                    pstm: ManuallyDrop::new(Some(self.contents_stream(format.lindex as usize)?)),
                },
                pUnkForRelease: ManuallyDrop::new(None),
            });
        }
        if format.cfFormat == self.drop_effect_format {
            if format.tymed & TYMED_HGLOBAL.0 as u32 == 0 {
                return Err(hresult_error(DV_E_TYMED));
            }
            let bytes = DROPEFFECT_COPY.0.to_le_bytes();
            return Ok(STGMEDIUM {
                tymed: TYMED_HGLOBAL.0 as u32,
                u: STGMEDIUM_0 {
                    hGlobal: global_from_bytes(&bytes)?,
                },
                pUnkForRelease: ManuallyDrop::new(None),
            });
        }
        Err(hresult_error(DV_E_FORMATETC))
    }

    /// 虚拟文件不支持由目标提供存储介质。
    fn GetDataHere(
        &self,
        _pformatetc: *const FORMATETC,
        _pmedium: *mut STGMEDIUM,
    ) -> WinResult<()> {
        Err(hresult_error(E_NOTIMPL))
    }

    /// 声明支持的文件描述符和内容格式。
    fn QueryGetData(&self, pformatetc: *const FORMATETC) -> HRESULT {
        let format = unsafe { &*pformatetc };
        if format.cfFormat == self.descriptor_format || format.cfFormat == self.drop_effect_format {
            return if format.tymed & TYMED_HGLOBAL.0 as u32 != 0 {
                S_OK
            } else {
                DV_E_TYMED
            };
        }
        if format.cfFormat == self.contents_format {
            if format.tymed & TYMED_ISTREAM.0 as u32 == 0 {
                return DV_E_TYMED;
            }
            if format.lindex < 0 || format.lindex as usize >= self.files.len() {
                return DV_E_LINDEX;
            }
            return S_OK;
        }
        DV_E_FORMATETC
    }

    /// 不支持规范化格式转换。
    fn GetCanonicalFormatEtc(
        &self,
        _pformatectin: *const FORMATETC,
        _pformatetcout: *mut FORMATETC,
    ) -> HRESULT {
        E_NOTIMPL
    }

    /// 拖放数据对象是只读的。
    fn SetData(
        &self,
        _pformatetc: *const FORMATETC,
        _pmedium: *const STGMEDIUM,
        _frelease: BOOL,
    ) -> WinResult<()> {
        Err(hresult_error(E_NOTIMPL))
    }

    /// 枚举拖放格式。
    fn EnumFormatEtc(&self, dwdirection: u32) -> WinResult<IEnumFORMATETC> {
        if dwdirection != DATADIR_GET.0 as u32 {
            return Err(hresult_error(E_NOTIMPL));
        }
        Ok(VirtualFormatEnum::new(self.formats.clone()).into())
    }

    /// 不支持拖放建议连接。
    fn DAdvise(
        &self,
        _pformatetc: *const FORMATETC,
        _advf: u32,
        _padvsink: Ref<'_, windows::Win32::System::Com::IAdviseSink>,
    ) -> WinResult<u32> {
        Err(hresult_error(OLE_E_ADVISENOTSUPPORTED))
    }

    /// 不支持拖放建议连接。
    fn DUnadvise(&self, _dwconnection: u32) -> WinResult<()> {
        Err(hresult_error(OLE_E_ADVISENOTSUPPORTED))
    }

    /// 不支持拖放建议连接。
    fn EnumDAdvise(&self) -> WinResult<windows::Win32::System::Com::IEnumSTATDATA> {
        Err(hresult_error(OLE_E_ADVISENOTSUPPORTED))
    }
}

/// 可枚举的拖放格式列表。
#[implement(IEnumFORMATETC)]
struct VirtualFormatEnum {
    formats: Vec<FORMATETC>,
    position: AtomicUsize,
}

impl VirtualFormatEnum {
    /// 从格式列表创建枚举器。
    fn new(formats: Vec<FORMATETC>) -> Self {
        Self {
            formats,
            position: AtomicUsize::new(0),
        }
    }
}

impl IEnumFORMATETC_Impl for VirtualFormatEnum_Impl {
    /// 复制当前游标处的一组格式。
    fn Next(&self, celt: u32, rgelt: *mut FORMATETC, pceltfetched: *mut u32) -> HRESULT {
        let start = self.position.load(Ordering::Relaxed);
        let remaining = self.formats.len().saturating_sub(start);
        let count = (celt as usize).min(remaining);
        if count > 0 && !rgelt.is_null() {
            unsafe {
                std::ptr::copy_nonoverlapping(self.formats.as_ptr().add(start), rgelt, count);
            }
        }
        self.position.store(start + count, Ordering::Relaxed);
        if !pceltfetched.is_null() {
            unsafe { *pceltfetched = count as u32 };
        }
        if count == celt as usize {
            S_OK
        } else {
            S_FALSE
        }
    }

    /// 跳过指定数量的格式。
    fn Skip(&self, celt: u32) -> WinResult<()> {
        let start = self.position.load(Ordering::Relaxed);
        self.position.store(
            (start + celt as usize).min(self.formats.len()),
            Ordering::Relaxed,
        );
        Ok(())
    }

    /// 回到第一个格式。
    fn Reset(&self) -> WinResult<()> {
        self.position.store(0, Ordering::Relaxed);
        Ok(())
    }

    /// 复制枚举器状态。
    fn Clone(&self) -> WinResult<IEnumFORMATETC> {
        Ok(VirtualFormatEnum {
            formats: self.formats.clone(),
            position: AtomicUsize::new(self.position.load(Ordering::Relaxed)),
        }
        .into())
    }
}

/// 延迟解压流的内部状态。
struct StreamState {
    /// 已解压的临时文件；`None` 表示尚未请求内容。
    file: Option<std::fs::File>,
    /// 临时目录，析构时删除解压出的文件。
    temp_dir: Option<tempfile::TempDir>,
    /// 当前读取位置。
    position: u64,
}

/// 首次读取时才解压的只读 IStream。
#[implement(IStream)]
struct VirtualStream {
    source: Arc<dyn VirtualFileSource>,
    index: usize,
    size: u64,
    state: Mutex<StreamState>,
}

impl VirtualStream {
    /// 为一组数据源和文件索引创建流。
    fn new(source: Arc<dyn VirtualFileSource>, index: usize, size: u64) -> Self {
        Self {
            source,
            index,
            size,
            state: Mutex::new(StreamState {
                file: None,
                temp_dir: None,
                position: 0,
            }),
        }
    }

    /// 确保条目已解压到临时文件，并在首次调用时打开它。
    fn ensure_file(&self, state: &mut StreamState) -> std::io::Result<()> {
        if state.file.is_some() {
            return Ok(());
        }
        let directory = tempfile::Builder::new().prefix("lumark-drag-").tempdir()?;
        let target = directory.path().join("payload");
        self.source
            .extract(self.index, &target)
            .map_err(std::io::Error::other)?;
        let file = std::fs::File::open(&target)?;
        state.file = Some(file);
        state.temp_dir = Some(directory);
        state.position = 0;
        Ok(())
    }
}

impl ISequentialStream_Impl for VirtualStream_Impl {
    /// 从解压后的临时文件顺序读取。
    fn Read(&self, pv: *mut core::ffi::c_void, cb: u32, pcbread: *mut u32) -> HRESULT {
        if pv.is_null() {
            return E_FAIL;
        }
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => return E_FAIL,
        };
        if let Err(error) = self.ensure_file(&mut state) {
            eprintln!("Lumark 虚拟拖放解压失败：{error}");
            return E_FAIL;
        }
        let position = state.position;
        let Some(file) = state.file.as_mut() else {
            return E_FAIL;
        };
        if let Err(error) = std::io::Seek::seek(file, std::io::SeekFrom::Start(position)) {
            eprintln!("Lumark 虚拟拖放定位失败：{error}");
            return E_FAIL;
        }
        let buffer = unsafe { std::slice::from_raw_parts_mut(pv.cast::<u8>(), cb as usize) };
        match file.read(buffer) {
            Ok(read) => {
                state.position = state.position.saturating_add(read as u64);
                if !pcbread.is_null() {
                    unsafe { *pcbread = read as u32 };
                }
                S_OK
            }
            Err(error) => {
                eprintln!("Lumark 虚拟拖放读取失败：{error}");
                E_FAIL
            }
        }
    }

    /// 拖放流只读。
    fn Write(&self, _pv: *const core::ffi::c_void, _cb: u32, _pcbwritten: *mut u32) -> HRESULT {
        E_NOTIMPL
    }
}

impl IStream_Impl for VirtualStream_Impl {
    /// 在声明的文件大小范围内移动读取位置。
    fn Seek(
        &self,
        dlibmove: i64,
        dworigin: STREAM_SEEK,
        plibnewposition: *mut u64,
    ) -> WinResult<()> {
        let mut state = self.state.lock().map_err(|_| hresult_error(E_FAIL))?;
        let base = match dworigin {
            STREAM_SEEK_SET => 0i128,
            STREAM_SEEK_CUR => i128::from(state.position),
            STREAM_SEEK_END => i128::from(self.size),
            _ => return Err(hresult_error(E_FAIL)),
        };
        let target = (base + i128::from(dlibmove)).clamp(0, i128::from(self.size)) as u64;
        state.position = target;
        if !plibnewposition.is_null() {
            unsafe { *plibnewposition = target };
        }
        Ok(())
    }

    /// 虚拟拖放流大小由归档条目固定。
    fn SetSize(&self, _libnewsize: u64) -> WinResult<()> {
        Err(hresult_error(E_NOTIMPL))
    }

    /// 不支持流到流的复制。
    fn CopyTo(
        &self,
        _pstm: Ref<'_, IStream>,
        _cb: u64,
        _pcbread: *mut u64,
        _pcbwritten: *mut u64,
    ) -> WinResult<()> {
        Err(hresult_error(E_NOTIMPL))
    }

    /// 只读流没有待提交的数据。
    fn Commit(&self, _grfcommitflags: &STGC) -> WinResult<()> {
        Ok(())
    }

    /// 不支持回滚。
    fn Revert(&self) -> WinResult<()> {
        Err(hresult_error(E_NOTIMPL))
    }

    /// 不支持区域锁定。
    fn LockRegion(&self, _liboffset: u64, _cb: u64, _dwlocktype: &LOCKTYPE) -> WinResult<()> {
        Err(hresult_error(E_NOTIMPL))
    }

    /// 不支持区域锁定。
    fn UnlockRegion(&self, _liboffset: u64, _cb: u64, _dwlocktype: u32) -> WinResult<()> {
        Err(hresult_error(E_NOTIMPL))
    }

    /// 返回流大小；资源管理器不要求名称。
    fn Stat(&self, pstatstg: *mut STATSTG, _grfstatflag: &STATFLAG) -> WinResult<()> {
        if pstatstg.is_null() {
            return Err(hresult_error(E_FAIL));
        }
        unsafe {
            *pstatstg = STATSTG {
                pwcsName: windows::core::PWSTR::null(),
                r#type: STGTY_STREAM.0 as u32,
                cbSize: self.size,
                mtime: FILETIME::default(),
                ctime: FILETIME::default(),
                atime: FILETIME::default(),
                grfMode: STGM(0),
                grfLocksSupported: 0,
                clsid: windows::core::GUID::zeroed(),
                grfStateBits: 0,
                reserved: 0,
            };
        }
        Ok(())
    }

    /// 复制一个独立的流；新流会在首次读取时重新解压。
    fn Clone(&self) -> WinResult<IStream> {
        Ok(VirtualStream::new(self.source.clone(), self.index, self.size).into())
    }
}

/// 只处理按钮状态的最小拖放源。
#[implement(IDropSource)]
struct VirtualDropSource;

impl IDropSource_Impl for VirtualDropSource_Impl {
    /// 松开左键时完成拖放，按 Esc 时取消。
    fn QueryContinueDrag(&self, fescapepressed: BOOL, grfkeystate: MODIFIERKEYS_FLAGS) -> HRESULT {
        if fescapepressed.as_bool() {
            return DRAGDROP_S_CANCEL;
        }
        if (grfkeystate.0 & MK_LBUTTON.0) == 0 {
            return DRAGDROP_S_DROP;
        }
        S_OK
    }

    /// 使用系统默认拖放光标。
    fn GiveFeedback(&self, _dweffect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Ole::{OleInitialize, OleUninitialize, ReleaseStgMedium};

    /// 记录解压次数并写出固定内容的测试数据源。
    struct TestSource {
        calls: Mutex<Vec<usize>>,
    }

    impl VirtualFileSource for TestSource {
        /// 返回一个目录和一个文件。
        fn files(&self) -> Vec<VirtualFile> {
            vec![
                VirtualFile {
                    name: "folder".into(),
                    size: 0,
                    is_dir: true,
                    modified: None,
                },
                VirtualFile {
                    name: "folder/hello.txt".into(),
                    size: 5,
                    is_dir: false,
                    modified: None,
                },
            ]
        }

        /// 把固定内容写到目标路径。
        fn extract(&self, index: usize, destination: &Path) -> Result<(), String> {
            self.calls.lock().unwrap().push(index);
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            std::fs::write(destination, b"hello").map_err(|error| error.to_string())
        }
    }

    /// 初始化 OLE，测试结束后恢复。
    fn with_ole<T>(f: impl FnOnce() -> T) -> T {
        let initialized = unsafe { OleInitialize(None) }.is_ok();
        let result = f();
        if initialized {
            unsafe { OleUninitialize() };
        }
        result
    }

    /// 返回三种拖放格式的剪贴板 ID。
    fn format_ids() -> (u16, u16, u16) {
        (
            unsafe { RegisterClipboardFormatW(CFSTR_FILEDESCRIPTORW) } as u16,
            unsafe { RegisterClipboardFormatW(CFSTR_FILECONTENTS) } as u16,
            unsafe { RegisterClipboardFormatW(CFSTR_PREFERREDDROPEFFECT) } as u16,
        )
    }

    /// 验证文件组描述符包含目录和文件的大小/属性。
    #[test]
    fn exposes_file_group_descriptor() {
        with_ole(|| {
            let source = TestSource {
                calls: Mutex::new(Vec::new()),
            };
            let files = source.files();
            let object: IDataObject = VirtualDataObject::new(Arc::new(source), files).into();
            let (descriptor_format, _, _) = format_ids();
            let format = format_etc(descriptor_format, TYMED_HGLOBAL.0);
            let mut medium = unsafe { object.GetData(&format) }.unwrap();
            assert_eq!(medium.tymed, TYMED_HGLOBAL.0 as u32);
            let memory = unsafe { medium.u.hGlobal };
            let pointer = unsafe { GlobalLock(memory) } as *const u8;
            let count = unsafe { std::ptr::read_unaligned(pointer.cast::<u32>()) };
            assert_eq!(count, 2);
            let item_size = std::mem::size_of::<FILEDESCRIPTORW>();
            let first =
                unsafe { std::ptr::read_unaligned(pointer.add(4).cast::<FILEDESCRIPTORW>()) };
            let first_attributes = first.dwFileAttributes;
            assert_ne!(first_attributes & FILE_ATTRIBUTE_DIRECTORY.0, 0);
            let second = unsafe {
                std::ptr::read_unaligned(pointer.add(4 + item_size).cast::<FILEDESCRIPTORW>())
            };
            let second_size = second.nFileSizeLow;
            assert_eq!(second_size, 5);
            unsafe {
                let _ = GlobalUnlock(memory);
                ReleaseStgMedium(&mut medium);
            }
        });
    }

    /// 验证 FileContents 只在真正读取时才解压。
    #[test]
    fn file_contents_extracts_lazily() {
        with_ole(|| {
            let source = Arc::new(TestSource {
                calls: Mutex::new(Vec::new()),
            });
            let files = source.files();
            let object: IDataObject = VirtualDataObject::new(source.clone(), files).into();
            let (_, contents_format, _) = format_ids();
            let mut format = format_etc(contents_format, TYMED_ISTREAM.0);
            format.lindex = 1;
            let mut medium = unsafe { object.GetData(&format) }.unwrap();
            assert!(source.calls.lock().unwrap().is_empty());
            let stream = unsafe { (*medium.u.pstm).as_ref().unwrap() };
            let mut buffer = [0u8; 5];
            let mut read = 0u32;
            let result = unsafe {
                stream.Read(
                    buffer.as_mut_ptr().cast(),
                    buffer.len() as u32,
                    Some(&mut read),
                )
            };
            assert!(result.is_ok());
            assert_eq!(read, 5);
            assert_eq!(&buffer, b"hello");
            assert_eq!(*source.calls.lock().unwrap(), vec![1]);
            unsafe { ReleaseStgMedium(&mut medium) };
        });
    }

    /// 验证格式查询接受支持的格式并拒绝错误类型。
    #[test]
    fn query_get_data_accepts_supported_formats() {
        with_ole(|| {
            let source = TestSource {
                calls: Mutex::new(Vec::new()),
            };
            let files = source.files();
            let object: IDataObject = VirtualDataObject::new(Arc::new(source), files).into();
            let (descriptor_format, contents_format, _) = format_ids();
            let descriptor = format_etc(descriptor_format, TYMED_HGLOBAL.0);
            assert_eq!(unsafe { object.QueryGetData(&descriptor) }, S_OK);

            let mut contents = format_etc(contents_format, TYMED_ISTREAM.0);
            contents.lindex = 1;
            assert_eq!(unsafe { object.QueryGetData(&contents) }, S_OK);

            let mut invalid = contents;
            invalid.lindex = 9;
            assert_eq!(unsafe { object.QueryGetData(&invalid) }, DV_E_LINDEX);

            let mut wrong_medium = contents;
            wrong_medium.tymed = TYMED_HGLOBAL.0 as u32;
            assert_eq!(unsafe { object.QueryGetData(&wrong_medium) }, DV_E_TYMED);

            let unknown = format_etc(0, TYMED_HGLOBAL.0);
            assert_eq!(unsafe { object.QueryGetData(&unknown) }, DV_E_FORMATETC);
        });
    }

    /// 验证枚举器只对 GET 方向返回格式。
    #[test]
    fn enum_format_etc_returns_get_formats() {
        with_ole(|| {
            let source = TestSource {
                calls: Mutex::new(Vec::new()),
            };
            let files = source.files();
            let object: IDataObject = VirtualDataObject::new(Arc::new(source), files).into();
            assert!(unsafe { object.EnumFormatEtc(DATADIR_GET.0 as u32) }.is_ok());
            assert!(unsafe { object.EnumFormatEtc(0) }.is_err());
        });
    }
}
