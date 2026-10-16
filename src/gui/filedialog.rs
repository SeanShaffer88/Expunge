//! Windows' own Open / Save / Choose-folder dialogs (IFileOpenDialog,
//! IFileSaveDialog) called through their COM vtables directly.

use std::ffi::c_void;
use std::path::PathBuf;
use windows_sys::core::{GUID, HRESULT};
use windows_sys::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
};

const CLSID_FILE_OPEN: GUID = GUID::from_u128(0xDC1C5A9C_E88A_4DDE_A5A1_60F82A20AEF7);
const CLSID_FILE_SAVE: GUID = GUID::from_u128(0xC0B4E2F3_BA21_4773_8DBA_335EC946EB8B);
const IID_FILE_DIALOG: GUID = GUID::from_u128(0x42F85136_DB7E_439C_85F1_E4075D135FC8);

const FOS_OVERWRITEPROMPT: u32 = 0x2;
const FOS_PICKFOLDERS: u32 = 0x20;
const FOS_FORCEFILESYSTEM: u32 = 0x40;
const FOS_PATHMUSTEXIST: u32 = 0x800;
const FOS_FILEMUSTEXIST: u32 = 0x1000;
const SIGDN_FILESYSPATH: i32 = 0x8005_8000u32 as i32;

#[repr(C)]
struct FilterSpec {
    name: *const u16,
    spec: *const u16,
}

type Raw = *mut c_void;

/// IFileDialog's vtable (IUnknown, IModalWindow, then its own methods in
/// declaration order). Methods not called are kept as plain slots.
#[repr(C)]
struct FileDialogVtbl {
    query_interface: usize,
    add_ref: usize,
    release: unsafe extern "system" fn(Raw) -> u32,
    show: unsafe extern "system" fn(Raw, *mut c_void) -> HRESULT,
    set_file_types: unsafe extern "system" fn(Raw, u32, *const FilterSpec) -> HRESULT,
    set_file_type_index: usize,
    get_file_type_index: usize,
    advise: usize,
    unadvise: usize,
    set_options: unsafe extern "system" fn(Raw, u32) -> HRESULT,
    get_options: unsafe extern "system" fn(Raw, *mut u32) -> HRESULT,
    set_default_folder: usize,
    set_folder: usize,
    get_folder: usize,
    get_current_selection: usize,
    set_file_name: unsafe extern "system" fn(Raw, *const u16) -> HRESULT,
    get_file_name: usize,
    set_title: unsafe extern "system" fn(Raw, *const u16) -> HRESULT,
    set_ok_button_label: usize,
    set_file_name_label: usize,
    get_result: unsafe extern "system" fn(Raw, *mut Raw) -> HRESULT,
    add_place: usize,
    set_default_extension: unsafe extern "system" fn(Raw, *const u16) -> HRESULT,
}

/// IShellItem's vtable.
#[repr(C)]
struct ShellItemVtbl {
    query_interface: usize,
    add_ref: usize,
    release: unsafe extern "system" fn(Raw) -> u32,
    bind_to_handler: usize,
    get_parent: usize,
    get_display_name: unsafe extern "system" fn(Raw, i32, *mut *mut u16) -> HRESULT,
}

/// A COM pointer released when dropped.
struct Com<V> {
    ptr: Raw,
    _v: std::marker::PhantomData<V>,
}

impl<V> Com<V> {
    fn vtbl(&self) -> &V {
        // SAFETY: a COM object starts with a pointer to its vtable, and `V`
        // matches the interface this pointer was obtained for.
        unsafe { &**(self.ptr as *const *const V) }
    }
}

trait Release {
    unsafe fn release(ptr: Raw);
}
impl Release for FileDialogVtbl {
    unsafe fn release(ptr: Raw) {
        let v = &**(ptr as *const *const FileDialogVtbl);
        (v.release)(ptr);
    }
}
impl Release for ShellItemVtbl {
    unsafe fn release(ptr: Raw) {
        let v = &**(ptr as *const *const ShellItemVtbl);
        (v.release)(ptr);
    }
}

impl<V: Release> Drop for Com<V> {
    fn drop(&mut self) {
        // SAFETY: we own one reference to a live object.
        unsafe { V::release(self.ptr) }
    }
}

/// COM for this thread, undone on drop only if this call set it up.
struct ComInit(bool);
impl ComInit {
    fn new() -> ComInit {
        // SAFETY: plain initialisation; S_FALSE means it was already set up
        // the same way (and still needs a matching uninitialise), a failure
        // (another apartment mode) leaves the existing setup in place.
        let hr = unsafe {
            CoInitializeEx(
                std::ptr::null(),
                (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
            )
        };
        ComInit(hr >= 0)
    }
}
impl Drop for ComInit {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: balances the successful CoInitializeEx above.
            unsafe { CoUninitialize() }
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

enum Kind<'a> {
    Open {
        filter: &'a str,
        exts: &'a [&'a str],
    },
    Folder,
    Save {
        name: &'a str,
        filter: &'a str,
        ext: &'a str,
    },
}

fn run(title: &str, kind: Kind) -> Option<PathBuf> {
    let _com = ComInit::new();
    let clsid = match kind {
        Kind::Save { .. } => &CLSID_FILE_SAVE,
        _ => &CLSID_FILE_OPEN,
    };
    let mut ptr: Raw = std::ptr::null_mut();
    // SAFETY: standard object creation; `ptr` receives an IFileDialog.
    let hr = unsafe {
        CoCreateInstance(
            clsid,
            std::ptr::null_mut(),
            CLSCTX_INPROC_SERVER,
            &IID_FILE_DIALOG,
            &mut ptr,
        )
    };
    if hr < 0 || ptr.is_null() {
        return None;
    }
    let dlg: Com<FileDialogVtbl> = Com {
        ptr,
        _v: std::marker::PhantomData,
    };
    let v = dlg.vtbl();
    let title_w = wide(title);
    // Strings and specs must outlive Show().
    let (filter_name, spec, default_name, default_ext);
    // SAFETY: each call passes the object's own pointer and buffers that
    // live until the end of this function.
    unsafe {
        let mut opts = 0u32;
        (v.get_options)(dlg.ptr, &mut opts);
        opts |= FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST;
        match &kind {
            Kind::Open { filter, exts } => {
                opts |= FOS_FILEMUSTEXIST;
                filter_name = wide(filter);
                let joined: Vec<String> = exts.iter().map(|e| format!("*.{e}")).collect();
                spec = wide(&joined.join(";"));
                let specs = [FilterSpec {
                    name: filter_name.as_ptr(),
                    spec: spec.as_ptr(),
                }];
                (v.set_file_types)(dlg.ptr, 1, specs.as_ptr());
            }
            Kind::Folder => opts |= FOS_PICKFOLDERS,
            Kind::Save { name, filter, ext } => {
                opts |= FOS_OVERWRITEPROMPT;
                filter_name = wide(filter);
                spec = wide(&format!("*.{ext}"));
                let specs = [FilterSpec {
                    name: filter_name.as_ptr(),
                    spec: spec.as_ptr(),
                }];
                (v.set_file_types)(dlg.ptr, 1, specs.as_ptr());
                default_name = wide(name);
                (v.set_file_name)(dlg.ptr, default_name.as_ptr());
                default_ext = wide(ext);
                (v.set_default_extension)(dlg.ptr, default_ext.as_ptr());
            }
        }
        (v.set_options)(dlg.ptr, opts);
        (v.set_title)(dlg.ptr, title_w.as_ptr());
        // The window in front owns the dialog, so it stays on top of it.
        let owner = windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow();
        if (v.show)(dlg.ptr, owner as *mut c_void) < 0 {
            return None; // cancelled
        }
        let mut item: Raw = std::ptr::null_mut();
        if (v.get_result)(dlg.ptr, &mut item) < 0 || item.is_null() {
            return None;
        }
        let item: Com<ShellItemVtbl> = Com {
            ptr: item,
            _v: std::marker::PhantomData,
        };
        let mut path: *mut u16 = std::ptr::null_mut();
        if (item.vtbl().get_display_name)(item.ptr, SIGDN_FILESYSPATH, &mut path) < 0
            || path.is_null()
        {
            return None;
        }
        let len = (0..).take_while(|&i| *path.add(i) != 0).count();
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(path, len));
        CoTaskMemFree(path as *const c_void);
        Some(PathBuf::from(s))
    }
}

pub fn pick_file(title: &str, filter: &str, exts: &[&str]) -> Option<PathBuf> {
    run(title, Kind::Open { filter, exts })
}

pub fn pick_folder(title: &str) -> Option<PathBuf> {
    run(title, Kind::Folder)
}

pub fn save_file(title: &str, name: &str, filter: &str, ext: &str) -> Option<PathBuf> {
    run(title, Kind::Save { name, filter, ext })
}
