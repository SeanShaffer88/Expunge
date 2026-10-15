//! Windows implementations: registry access, process launching, elevation.

use super::Registry;
use crate::model::{fold, KeyData, RegValue};
use anyhow::{anyhow, Context, Result};
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use winreg::enums::*;
use winreg::RegKey;

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, HANDLE, WAIT_FAILED, WAIT_OBJECT_0,
};

/// A kernel handle that is closed when it goes out of scope, on every path
/// (early returns, errors, panics).
struct OwnedHandle(HANDLE);

impl OwnedHandle {
    /// Wrap a handle returned by Windows; `None` if the call failed.
    fn new(h: HANDLE) -> Option<Self> {
        (!h.is_null() && h != INVALID_HANDLE).then_some(OwnedHandle(h))
    }
    fn get(&self) -> HANDLE {
        self.0
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

const INVALID_HANDLE: HANDLE = -1isize as HANDLE;
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
};
use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_DELAY_UNTIL_REBOOT};
use windows_sys::Win32::System::Registry::RegSetValueExW;
use windows_sys::Win32::System::Threading::{
    CreateProcessW, GetCurrentProcess, GetExitCodeProcess, OpenProcessToken, WaitForSingleObject,
    PROCESS_INFORMATION, STARTUPINFOW,
};
use windows_sys::Win32::UI::Shell::{
    ShellExecuteExW, ShellExecuteW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

const ERROR_ELEVATION_REQUIRED: u32 = 740;

pub fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn hive(short: &str) -> RegKey {
    let h = match short {
        "HKLM" => HKEY_LOCAL_MACHINE,
        "HKCU" => HKEY_CURRENT_USER,
        "HKU" => HKEY_USERS,
        "HKCR" => HKEY_CLASSES_ROOT,
        _ => HKEY_CURRENT_CONFIG,
    };
    RegKey::predef(h)
}

fn open(path: &str, access: u32) -> std::io::Result<RegKey> {
    let (h, rest) = super::split_hive(path)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "unknown hive"))?;
    let root = hive(h);
    if rest.is_empty() {
        return Ok(root);
    }
    root.open_subkey_with_flags(rest, access | KEY_WOW64_64KEY)
}

/// The real Windows registry (64-bit view).
pub struct WinRegistry;

impl Registry for WinRegistry {
    fn exists(&self, path: &str) -> bool {
        open(path, KEY_READ).is_ok()
    }

    fn read_key(&self, path: &str) -> Option<KeyData> {
        let key = open(path, KEY_QUERY_VALUE).ok()?;
        let mut kd = KeyData::new(path);
        for (name, v) in key.enum_values().flatten() {
            let rv = RegValue {
                name: name.clone(),
                kind: v.vtype.clone() as u32,
                data: v.bytes.to_vec(),
            };
            kd.values.insert(fold(&name), rv);
        }
        Some(kd)
    }

    fn subkeys(&self, path: &str) -> Vec<String> {
        match open(path, KEY_ENUMERATE_SUB_KEYS) {
            Ok(k) => k.enum_keys().flatten().collect(),
            Err(_) => Vec::new(),
        }
    }

    fn create_key(&self, path: &str) -> Result<()> {
        let (h, rest) = super::split_hive(path).ok_or_else(|| anyhow!("unknown hive: {path}"))?;
        if rest.is_empty() {
            return Ok(());
        }
        hive(h)
            .create_subkey_with_flags(rest, KEY_ALL_ACCESS | KEY_WOW64_64KEY)
            .with_context(|| format!("create {path}"))?;
        Ok(())
    }

    fn set_value(&self, key: &str, value: &RegValue) -> Result<()> {
        self.create_key(key)?;
        let k = open(key, KEY_SET_VALUE).with_context(|| format!("open {key}"))?;
        let name = wide(&value.name);
        // Raw write keeps the exact original type, including unusual ones.
        let rc = unsafe {
            RegSetValueExW(
                k.raw_handle() as _,
                name.as_ptr(),
                0,
                value.kind,
                value.data.as_ptr(),
                value.data.len() as u32,
            )
        };
        if rc != 0 {
            return Err(anyhow!(
                "RegSetValueEx {key}\\{} failed: {}",
                value.name,
                std::io::Error::from_raw_os_error(rc as i32)
            ));
        }
        Ok(())
    }

    fn delete_value(&self, key: &str, name: &str) -> Result<()> {
        let k = open(key, KEY_SET_VALUE).with_context(|| format!("open {key}"))?;
        k.delete_value(name)
            .with_context(|| format!("delete value {key}\\{name}"))?;
        Ok(())
    }

    fn delete_tree(&self, path: &str) -> Result<()> {
        let (h, rest) = super::split_hive(path).ok_or_else(|| anyhow!("unknown hive: {path}"))?;
        if rest.is_empty() {
            return Err(anyhow!("refusing to delete a hive root"));
        }
        // The last line of defence, whoever asked.
        if crate::util::is_protected_key(path) {
            return Err(anyhow!("refusing to delete protected key {path}"));
        }
        let (parent, leaf) = match rest.rsplit_once('\\') {
            Some((p, l)) => (format!("{h}\\{p}"), l.to_string()),
            None => (h.to_string(), rest.to_string()),
        };
        let pk = open(&parent, KEY_ALL_ACCESS).with_context(|| format!("open {parent}"))?;
        pk.delete_subkey_all(&leaf)
            .with_context(|| format!("delete {path}"))?;
        Ok(())
    }
}

pub fn delete_on_reboot(path: &Path) -> Result<()> {
    let p = wide(&path.to_string_lossy());
    let ok = unsafe { MoveFileExW(p.as_ptr(), std::ptr::null(), MOVEFILE_DELAY_UNTIL_REBOOT) };
    if ok == 0 {
        return Err(anyhow!(
            "MoveFileEx failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

pub fn is_elevated() -> bool {
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let Some(token) = OwnedHandle::new(token) else {
            return false;
        };
        let mut elev = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token.get(),
            TokenElevation,
            &mut elev as *mut _ as *mut _,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        ok != 0 && elev.TokenIsElevated != 0
    }
}

/// A process started by Expunge (installer, uninstaller, tool).
pub struct ChildProcess {
    handle: HANDLE,
}

unsafe impl Send for ChildProcess {}

impl ChildProcess {
    /// `Some(exit_code)` once the process has ended.
    pub fn try_wait(&self) -> Option<u32> {
        if self.handle.is_null() {
            return Some(0);
        }
        unsafe {
            match WaitForSingleObject(self.handle, 0) {
                WAIT_OBJECT_0 => {
                    let mut code = 0u32;
                    if GetExitCodeProcess(self.handle, &mut code) == 0 {
                        code = GetLastError();
                    }
                    Some(code)
                }
                // The handle can't be waited on: report it as ended with the
                // error, rather than waiting forever.
                WAIT_FAILED => Some(GetLastError().max(1)),
                _ => None,
            }
        }
    }

    pub fn wait(&self) -> u32 {
        loop {
            if let Some(c) = self.try_wait() {
                return c;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    }
}

impl Drop for ChildProcess {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { CloseHandle(self.handle) };
        }
    }
}

/// Run a raw command line exactly as Windows would (like the Run dialog),
/// elevating through UAC if the target requires it.
pub fn spawn_command_line(cmdline: &str) -> Result<ChildProcess> {
    let mut buf = wide(cmdline);
    unsafe {
        let mut si: STARTUPINFOW = std::mem::zeroed();
        si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
        let ok = CreateProcessW(
            std::ptr::null(),
            buf.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            &si,
            &mut pi,
        );
        if ok != 0 {
            CloseHandle(pi.hThread);
            return Ok(ChildProcess {
                handle: pi.hProcess,
            });
        }
        let err = GetLastError();
        if err == ERROR_ELEVATION_REQUIRED {
            let (prog, args) = crate::util::split_command_line(cmdline);
            return shell_execute_wait(&prog, &args, true);
        }
        Err(anyhow!(
            "could not start `{cmdline}`: {}",
            std::io::Error::from_raw_os_error(err as i32)
        ))
    }
}

/// ShellExecuteEx with an optional "runas" verb, returning a waitable handle.
pub fn shell_execute_wait(file: &str, params: &str, elevate: bool) -> Result<ChildProcess> {
    let verb = wide(if elevate { "runas" } else { "open" });
    let f = wide(file);
    let p = wide(params);
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb.as_ptr();
        info.lpFile = f.as_ptr();
        info.lpParameters = if params.is_empty() {
            std::ptr::null()
        } else {
            p.as_ptr()
        };
        info.nShow = SW_SHOWNORMAL;
        if ShellExecuteExW(&mut info) == 0 {
            return Err(anyhow!(
                "could not start {file}: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(ChildProcess {
            handle: info.hProcess,
        })
    }
}

/// Open a file, folder or URL with its default handler.
pub fn shell_open(target: &str) {
    let verb = wide("open");
    let t = wide(target);
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            t.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
    }
}

/// Restart Expunge elevated with the given arguments. Returns true if the new
/// process launched.
pub fn relaunch_elevated(args: &[String]) -> bool {
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let params = args
        .iter()
        .map(|a| {
            if a.contains(' ') || a.is_empty() {
                format!("\"{a}\"")
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    shell_execute_wait(&exe, &params, true).is_ok()
}

/// Attach to the parent console so CLI output is visible from cmd/PowerShell
/// even though the exe uses the GUI subsystem.
pub fn attach_parent_console() {
    use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

/// Open Registry Editor at the given key.
pub fn open_in_regedit(path: &str) -> Result<()> {
    let (h, rest) = super::split_hive(path).ok_or_else(|| anyhow!("unknown hive"))?;
    let long = super::HIVES
        .iter()
        .find(|(s, _)| *s == h)
        .map(|(_, l)| *l)
        .unwrap_or(h);
    let target = if rest.is_empty() {
        format!("Computer\\{long}")
    } else {
        format!("Computer\\{long}\\{rest}")
    };
    let applet = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Applets\\Regedit")?
        .0;
    applet.set_value("LastKey", &target)?;
    // Regedit is single-instance; /m opens a new window that honours LastKey.
    shell_execute_wait("regedit.exe", "/m", false)?;
    Ok(())
}

/// (items, bytes) in the Recycle Bin across all drives.
pub fn recycle_bin_info() -> (u64, u64) {
    use windows_sys::Win32::UI::Shell::{SHQueryRecycleBinW, SHQUERYRBINFO};
    unsafe {
        let mut info: SHQUERYRBINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHQUERYRBINFO>() as u32;
        if SHQueryRecycleBinW(std::ptr::null(), &mut info) == 0 {
            (info.i64NumItems.max(0) as u64, info.i64Size.max(0) as u64)
        } else {
            (0, 0)
        }
    }
}

pub fn empty_recycle_bin() -> bool {
    use windows_sys::Win32::UI::Shell::{
        SHEmptyRecycleBinW, SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND,
    };
    unsafe {
        SHEmptyRecycleBinW(
            std::ptr::null_mut(),
            std::ptr::null(),
            SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND,
        ) == 0
    }
}

/// Windows display locale, e.g. `de-DE`.
pub fn user_locale() -> String {
    use windows_sys::Win32::Globalization::GetUserDefaultLocaleName;
    let mut buf = [0u16; 85];
    let n = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
    if n > 1 {
        String::from_utf16_lossy(&buf[..(n as usize - 1)])
    } else {
        "en-US".into()
    }
}

/// Give every visible top-level window of this process a native Windows 11
/// border colour and a title bar that matches the light/dark theme.
pub fn apply_window_style(border_rgb: (u8, u8, u8), dark: bool) {
    use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible,
    };

    const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
    const DWMWA_BORDER_COLOR: u32 = 34;

    struct Ctx {
        pid: u32,
        color: u32,
        dark: i32,
    }
    unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = &*(lparam as *const Ctx);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == ctx.pid && IsWindowVisible(hwnd) != 0 {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_BORDER_COLOR,
                &ctx.color as *const u32 as *const _,
                4,
            );
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &ctx.dark as *const i32 as *const _,
                4,
            );
        }
        1
    }
    let (r, g, b) = border_rgb;
    // COLORREF is 0x00BBGGRR.
    let ctx = Ctx {
        pid: unsafe { GetCurrentProcessId() },
        color: (b as u32) << 16 | (g as u32) << 8 | r as u32,
        dark: dark as i32,
    };
    unsafe {
        EnumWindows(Some(each), &ctx as *const Ctx as LPARAM);
    }
}

/// A plain Windows message box (used only when the window can't be created).
pub fn message_box(title: &str, text: &str) -> i32 {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
    let (t, m) = (wide(title), wide(text));
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            m.as_ptr(),
            t.as_ptr(),
            MB_OK | MB_ICONERROR,
        )
    }
}

/// Windows' "Animation effects" accessibility setting.
pub fn client_area_animation() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION,
    };
    let mut on: i32 = 1;
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            &mut on as *mut i32 as *mut _,
            0,
        )
    };
    ok == 0 || on != 0
}

/// An information message box that stays on top until OK is clicked.
pub fn info_box(title: &str, text: &str) -> i32 {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, MB_ICONINFORMATION, MB_OK, MB_SETFOREGROUND, MB_TOPMOST,
    };
    let (t, m) = (wide(title), wide(text));
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            m.as_ptr(),
            t.as_ptr(),
            MB_OK | MB_ICONINFORMATION | MB_SETFOREGROUND | MB_TOPMOST,
        )
    }
}

/// Move a file or folder to the Recycle Bin, silently.
pub fn recycle(path: &Path) -> Result<()> {
    use windows_sys::Win32::UI::Shell::{
        SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT,
        FOF_WANTNUKEWARNING, FO_DELETE, SHFILEOPSTRUCTW,
    };
    // pFrom must be double-NUL terminated.
    let mut from = wide(&path.to_string_lossy());
    from.push(0);
    unsafe {
        let mut op: SHFILEOPSTRUCTW = std::mem::zeroed();
        op.wFunc = FO_DELETE as _;
        op.pFrom = from.as_ptr();
        op.fFlags =
            (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI | FOF_WANTNUKEWARNING)
                as _;
        let rc = SHFileOperationW(&mut op);
        if rc != 0 || op.fAnyOperationsAborted != 0 || path.exists() {
            return Err(anyhow!(
                "could not move {} to the Recycle Bin (code {rc})",
                path.display()
            ));
        }
    }
    Ok(())
}

/// Extract an icon from an .exe, .dll or .ico as RGBA pixels.
pub fn icon_rgba(path: &str, index: i32, size: u32) -> Option<(u32, u32, Vec<u8>)> {
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, BITMAPINFO, BITMAPINFOHEADER,
        BI_RGB, DIB_RGB_COLORS,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DestroyIcon, GetIconInfo, PrivateExtractIconsW, HICON, ICONINFO,
    };
    if !Path::new(path).exists() {
        return None;
    }
    let wpath = wide(path);
    let mut hicon: HICON = std::ptr::null_mut();
    let mut id = 0u32;
    unsafe {
        let n = PrivateExtractIconsW(
            wpath.as_ptr(),
            index,
            size as i32,
            size as i32,
            &mut hicon,
            &mut id,
            1,
            0,
        );
        if n == 0 || n == u32::MAX || hicon.is_null() {
            return None;
        }
        let mut info: ICONINFO = std::mem::zeroed();
        if GetIconInfo(hicon, &mut info) == 0 {
            DestroyIcon(hicon);
            return None;
        }
        let dc = CreateCompatibleDC(std::ptr::null_mut());
        let mut bmi: BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = size as i32;
        bmi.bmiHeader.biHeight = -(size as i32); // top-down
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB;
        let mut px = vec![0u8; (size * size * 4) as usize];
        let ok = !info.hbmColor.is_null()
            && GetDIBits(
                dc,
                info.hbmColor,
                0,
                size,
                px.as_mut_ptr() as *mut _,
                &mut bmi,
                DIB_RGB_COLORS,
            ) != 0;
        let has_alpha = px.chunks_exact(4).any(|p| p[3] != 0);
        if ok && !has_alpha {
            // Old-style icon without alpha: derive transparency from the mask.
            let mut mask = vec![0u8; (size * size * 4) as usize];
            if GetDIBits(
                dc,
                info.hbmMask,
                0,
                size,
                mask.as_mut_ptr() as *mut _,
                &mut bmi,
                DIB_RGB_COLORS,
            ) != 0
            {
                for (p, m) in px.chunks_exact_mut(4).zip(mask.chunks_exact(4)) {
                    p[3] = if m[0] == 0 { 255 } else { 0 };
                }
            }
        }
        DeleteDC(dc);
        if !info.hbmColor.is_null() {
            DeleteObject(info.hbmColor);
        }
        if !info.hbmMask.is_null() {
            DeleteObject(info.hbmMask);
        }
        DestroyIcon(hicon);
        if !ok {
            return None;
        }
        // BGRA -> RGBA
        for p in px.chunks_exact_mut(4) {
            p.swap(0, 2);
        }
        Some((size, size, px))
    }
}

/// Verify a file's Authenticode signature offline. Returns the WinVerifyTrust
/// result (0 = valid) and the signer's display name when it can be read.
pub fn authenticode(path: &str) -> (i32, Option<String>) {
    use windows_sys::Win32::Security::WinTrust::{
        WinVerifyTrust, WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0,
        WINTRUST_FILE_INFO, WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_REVOKE_NONE,
        WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE,
    };
    let wpath = wide(path);
    let code = unsafe {
        let mut file: WINTRUST_FILE_INFO = std::mem::zeroed();
        file.cbStruct = std::mem::size_of::<WINTRUST_FILE_INFO>() as u32;
        file.pcwszFilePath = wpath.as_ptr();
        let mut data: WINTRUST_DATA = std::mem::zeroed();
        data.cbStruct = std::mem::size_of::<WINTRUST_DATA>() as u32;
        data.dwUIChoice = WTD_UI_NONE;
        data.fdwRevocationChecks = WTD_REVOKE_NONE;
        data.dwUnionChoice = WTD_CHOICE_FILE;
        data.Anonymous = WINTRUST_DATA_0 { pFile: &mut file };
        data.dwStateAction = WTD_STATEACTION_VERIFY;
        data.dwProvFlags = WTD_CACHE_ONLY_URL_RETRIEVAL;
        let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
        let rc = WinVerifyTrust(
            std::ptr::null_mut(),
            &mut action,
            &mut data as *mut _ as *mut _,
        );
        data.dwStateAction = WTD_STATEACTION_CLOSE;
        WinVerifyTrust(
            std::ptr::null_mut(),
            &mut action,
            &mut data as *mut _ as *mut _,
        );
        rc
    };
    (code, signer_name(&wpath))
}

/// Read the signer's display name from a file's embedded signature.
fn signer_name(wpath: &[u16]) -> Option<String> {
    use windows_sys::Win32::Security::Cryptography::{
        CertCloseStore, CertFindCertificateInStore, CertFreeCertificateContext, CertGetNameStringW,
        CryptMsgClose, CryptMsgGetParam, CryptQueryObject, CERT_FIND_SUBJECT_CERT, CERT_INFO,
        CERT_NAME_SIMPLE_DISPLAY_TYPE, CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED,
        CERT_QUERY_FORMAT_FLAG_BINARY, CERT_QUERY_OBJECT_FILE, CMSG_SIGNER_INFO,
        CMSG_SIGNER_INFO_PARAM, PKCS_7_ASN_ENCODING, X509_ASN_ENCODING,
    };
    unsafe {
        let mut encoding = 0u32;
        let mut content = 0u32;
        let mut format = 0u32;
        let mut store = std::ptr::null_mut();
        let mut msg = std::ptr::null_mut();
        let ok = CryptQueryObject(
            CERT_QUERY_OBJECT_FILE,
            wpath.as_ptr() as *const _,
            CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED,
            CERT_QUERY_FORMAT_FLAG_BINARY,
            0,
            &mut encoding,
            &mut content,
            &mut format,
            &mut store,
            &mut msg,
            std::ptr::null_mut(),
        );
        if ok == 0 {
            return None;
        }
        let mut name = None;
        let mut size = 0u32;
        if CryptMsgGetParam(
            msg,
            CMSG_SIGNER_INFO_PARAM,
            0,
            std::ptr::null_mut(),
            &mut size,
        ) != 0
            && size > 0
        {
            // u64 buffer keeps the struct properly aligned.
            let mut buf = vec![0u64; (size as usize).div_ceil(8)];
            if CryptMsgGetParam(
                msg,
                CMSG_SIGNER_INFO_PARAM,
                0,
                buf.as_mut_ptr() as *mut _,
                &mut size,
            ) != 0
            {
                let info = &*(buf.as_ptr() as *const CMSG_SIGNER_INFO);
                let mut cert_info: CERT_INFO = std::mem::zeroed();
                cert_info.Issuer = info.Issuer;
                cert_info.SerialNumber = info.SerialNumber;
                let cert = CertFindCertificateInStore(
                    store,
                    X509_ASN_ENCODING | PKCS_7_ASN_ENCODING,
                    0,
                    CERT_FIND_SUBJECT_CERT,
                    &cert_info as *const _ as *const _,
                    std::ptr::null(),
                );
                if !cert.is_null() {
                    let mut out = [0u16; 256];
                    let n = CertGetNameStringW(
                        cert,
                        CERT_NAME_SIMPLE_DISPLAY_TYPE,
                        0,
                        std::ptr::null(),
                        out.as_mut_ptr(),
                        out.len() as u32,
                    );
                    if n > 1 {
                        name = Some(String::from_utf16_lossy(&out[..(n as usize - 1)]));
                    }
                    CertFreeCertificateContext(cert);
                }
            }
        }
        CryptMsgClose(msg);
        CertCloseStore(store, 0);
        name
    }
}

/// True while a Windows Installer (MSI) operation is running anywhere on the
/// system. Only one can run at a time; starting another fails with 1618.
pub fn installer_busy() -> bool {
    use windows_sys::Win32::System::Threading::OpenMutexW;
    const SYNCHRONIZE: u32 = 0x0010_0000;
    let name: Vec<u16> = "Global\\_MSIExecute\0".encode_utf16().collect();
    unsafe { OwnedHandle::new(OpenMutexW(SYNCHRONIZE, 0, name.as_ptr())).is_some() }
}

/// Whether a process with this id is still running.
pub fn process_alive(pid: u32) -> bool {
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    const STILL_ACTIVE: u32 = 259;
    unsafe {
        let Some(h) = OwnedHandle::new(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid))
        else {
            return false;
        };
        let mut code = 0u32;
        GetExitCodeProcess(h.get(), &mut code) != 0 && code == STILL_ACTIVE
    }
}

/// Every running process this user can see: program file, memory in use
/// and total CPU time. Used for the resource-use history.
pub fn process_samples() -> Vec<crate::usage::ProcSample> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::ProcessStatus::{
        K32EnumProcesses, K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // Grow the buffer until the list fits (a full buffer may be truncated).
    let mut pids = vec![0u32; 4096];
    loop {
        let mut needed = 0u32;
        let bytes = (pids.len() * 4) as u32;
        if unsafe { K32EnumProcesses(pids.as_mut_ptr(), bytes, &mut needed) } == 0 {
            return Vec::new();
        }
        if needed < bytes || pids.len() >= 1 << 20 {
            pids.truncate(needed as usize / 4);
            break;
        }
        pids = vec![0u32; pids.len() * 2];
    }
    let ft = |f: FILETIME| ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64;
    let mut out = Vec::new();
    for pid in pids.into_iter().filter(|&p| p > 4) {
        unsafe {
            let Some(owned) =
                OwnedHandle::new(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid))
            else {
                continue;
            };
            let h = owned.get();
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let path = if QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len) != 0 {
                String::from_utf16_lossy(&buf[..(len as usize).min(buf.len())])
            } else {
                String::new()
            };
            let mut mem: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
            mem.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
            let ws = if K32GetProcessMemoryInfo(h, &mut mem, mem.cb) != 0 {
                mem.WorkingSetSize as u64
            } else {
                0
            };
            let (mut c, mut e, mut k, mut u): (FILETIME, FILETIME, FILETIME, FILETIME) =
                std::mem::zeroed();
            let cpu = if GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u) != 0 {
                (ft(k) + ft(u)) / 10_000
            } else {
                0
            };
            drop(owned);
            if !path.is_empty() {
                out.push(crate::usage::ProcSample {
                    path,
                    working_set: ws,
                    cpu_ms: cpu,
                });
            }
        }
    }
    out
}

/// Run this process at background priority (low CPU and disk priority).
pub fn background_priority() {
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, SetPriorityClass, PROCESS_MODE_BACKGROUND_BEGIN,
    };
    unsafe {
        SetPriorityClass(GetCurrentProcess(), PROCESS_MODE_BACKGROUND_BEGIN);
    }
}

/// Block until `stop` is set, calling `on_change` whenever anything under the
/// registry key `path` changes (keys added or removed, values written).
pub fn watch_registry(
    path: &str,
    stop: &std::sync::atomic::AtomicBool,
    mut on_change: impl FnMut(),
) -> Result<()> {
    use std::sync::atomic::Ordering;
    use windows_sys::Win32::System::Registry::{
        RegNotifyChangeKeyValue, REG_NOTIFY_CHANGE_LAST_SET, REG_NOTIFY_CHANGE_NAME,
    };
    use windows_sys::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
    let key = open(path, KEY_NOTIFY)?;
    let raw = key.raw_handle() as windows_sys::Win32::System::Registry::HKEY;
    unsafe {
        let Some(owned) = OwnedHandle::new(CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()))
        else {
            return Err(anyhow!("could not create an event"));
        };
        let event = owned.get();
        while !stop.load(Ordering::Relaxed) {
            let rc = RegNotifyChangeKeyValue(
                raw,
                1,
                REG_NOTIFY_CHANGE_NAME | REG_NOTIFY_CHANGE_LAST_SET,
                event,
                1,
            );
            if rc != 0 {
                return Err(anyhow!(
                    "registry notifications unavailable for {path} ({rc})"
                ));
            }
            loop {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                match WaitForSingleObject(event, 400) {
                    WAIT_OBJECT_0 => {
                        on_change();
                        break;
                    }
                    // Never spin on a wait that can't succeed.
                    WAIT_FAILED => {
                        return Err(anyhow!(
                            "waiting for registry changes failed for {path}: {}",
                            std::io::Error::last_os_error()
                        ))
                    }
                    _ => {}
                }
            }
        }
    }
    drop(key);
    Ok(())
}

/// Stop Windows showing "There is no disk in the drive" and similar
/// dialogs when Expunge touches an empty card reader or removed drive.
pub fn quiet_critical_errors() {
    #[link(name = "kernel32")]
    extern "system" {
        fn SetErrorMode(mode: u32) -> u32;
    }
    const SEM_FAILCRITICALERRORS: u32 = 0x0001;
    const SEM_NOOPENFILEERRORBOX: u32 = 0x8000;
    unsafe {
        SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOOPENFILEERRORBOX);
    }
}

/// Lower the current thread's CPU priority a little, so background work
/// never makes the window or other programs stutter.
pub fn lower_thread_priority() {
    use windows_sys::Win32::System::Threading::{
        GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
    };
    unsafe {
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
}

/// Delete one System Restore point (srclient.dll's SRRemoveRestorePoint).
pub fn remove_restore_point(sequence: u32) -> Result<()> {
    use windows_sys::Win32::Foundation::FreeLibrary;
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    let name = wide("srclient.dll");
    unsafe {
        let lib = LoadLibraryW(name.as_ptr());
        if lib.is_null() {
            return Err(anyhow!("System Restore is not available"));
        }
        let Some(f) = GetProcAddress(lib, b"SRRemoveRestorePoint\0".as_ptr()) else {
            FreeLibrary(lib);
            return Err(anyhow!("System Restore is not available"));
        };
        let remove: unsafe extern "system" fn(u32) -> u32 = std::mem::transmute(f);
        let rc = remove(sequence);
        FreeLibrary(lib);
        match rc {
            0 => Ok(()),
            code => Err(anyhow!(
                "Windows could not delete the restore point (error {code})"
            )),
        }
    }
}

/// Total size of a drive in bytes.
pub fn drive_size(root: &str) -> Option<u64> {
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let w = wide(root);
    let (mut a, mut t, mut f) = (0u64, 0u64, 0u64);
    unsafe { (GetDiskFreeSpaceExW(w.as_ptr(), &mut a, &mut t, &mut f) != 0).then_some(t) }
}

// ---------------------------------------------------------------------------
// Files held open: Restart Manager
// ---------------------------------------------------------------------------

use windows_sys::Win32::System::RestartManager::{
    RmEndSession, RmGetList, RmRegisterResources, RmRestart, RmShutdown, RmStartSession,
    RM_PROCESS_INFO, RM_UNIQUE_PROCESS,
};

const ERROR_MORE_DATA: u32 = 234;
const RM_SESSION_KEY_LEN: usize = 32;
const RM_FORCE_SHUTDOWN: u32 = 0x1;
// RM_APP_TYPE values.
const RM_SERVICE: i32 = 3;
const RM_EXPLORER: i32 = 4;
const RM_CRITICAL: i32 = 1000;
/// Paths registered with one Restart Manager session at most.
const RM_MAX_FILES: usize = 4096;

/// A Restart Manager session, ended when dropped.
struct RmSession(u32);

impl RmSession {
    fn start() -> Option<Self> {
        let mut handle = 0u32;
        let mut key = [0u16; RM_SESSION_KEY_LEN + 1];
        let rc = unsafe { RmStartSession(&mut handle, 0, key.as_mut_ptr()) };
        (rc == 0).then_some(RmSession(handle))
    }

    fn register_files(&self, paths: &[&Path]) -> bool {
        let wides: Vec<Vec<u16>> = paths
            .iter()
            .take(RM_MAX_FILES)
            .map(|p| wide(&p.to_string_lossy()))
            .collect();
        let ptrs: Vec<*const u16> = wides.iter().map(|w| w.as_ptr()).collect();
        let rc = unsafe {
            RmRegisterResources(
                self.0,
                ptrs.len() as u32,
                ptrs.as_ptr(),
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
            )
        };
        rc == 0
    }

    fn list(&self) -> Vec<RM_PROCESS_INFO> {
        let mut infos: Vec<RM_PROCESS_INFO> = Vec::new();
        // The list can grow between calls; retry a few times.
        for _ in 0..4 {
            let mut needed = 0u32;
            let mut count = infos.capacity() as u32;
            let mut reasons = 0u32;
            let rc = unsafe {
                RmGetList(
                    self.0,
                    &mut needed,
                    &mut count,
                    infos.as_mut_ptr(),
                    &mut reasons,
                )
            };
            match rc {
                0 => {
                    unsafe { infos.set_len(count as usize) };
                    return infos;
                }
                ERROR_MORE_DATA => {
                    infos = Vec::with_capacity(needed as usize + 4);
                }
                _ => return Vec::new(),
            }
        }
        Vec::new()
    }
}

impl Drop for RmSession {
    fn drop(&mut self) {
        unsafe { RmEndSession(self.0) };
    }
}

fn from_wide_buf(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

fn holder_from(info: &RM_PROCESS_INFO) -> crate::locks::Holder {
    use crate::locks::{Holder, HolderKind};
    let name = from_wide_buf(&info.strAppName);
    let service = from_wide_buf(&info.strServiceShortName);
    let kind = match info.ApplicationType {
        RM_EXPLORER => HolderKind::Explorer,
        RM_SERVICE => HolderKind::Service,
        RM_CRITICAL => HolderKind::System,
        _ => HolderKind::Program,
    };
    Holder {
        pid: info.Process.dwProcessId,
        name,
        service: (!service.is_empty()).then_some(service),
        kind,
    }
}

pub fn file_holders(paths: &[&Path]) -> Vec<crate::locks::Holder> {
    let Some(s) = RmSession::start() else {
        return Vec::new();
    };
    if !s.register_files(paths) {
        return Vec::new();
    }
    s.list().iter().map(holder_from).collect()
}

/// Windows Explorer shut down through the Restart Manager; restarted on drop.
pub struct ExplorerPause {
    session: RmSession,
}

impl Drop for ExplorerPause {
    fn drop(&mut self) {
        let rc = unsafe { RmRestart(self.session.0, 0, None) };
        if rc != 0 {
            // Restart Manager couldn't bring it back; start it ourselves so
            // the user isn't left without a taskbar.
            let _ = std::process::Command::new(expand_windir("explorer.exe")).spawn();
        }
    }
}

fn expand_windir(file: &str) -> String {
    match std::env::var("SystemRoot") {
        Ok(root) => format!("{root}\\{file}"),
        Err(_) => file.to_string(),
    }
}

pub fn pause_explorer(paths: &[&Path]) -> Option<ExplorerPause> {
    let query = RmSession::start()?;
    if !query.register_files(paths) {
        return None;
    }
    let explorers: Vec<RM_UNIQUE_PROCESS> = query
        .list()
        .iter()
        .filter(|i| i.ApplicationType == RM_EXPLORER)
        .map(|i| i.Process)
        .collect();
    drop(query);
    if explorers.is_empty() {
        return None;
    }
    // A second session that knows only Explorer, so nothing else that has
    // the files open is ever shut down.
    let session = RmSession::start()?;
    let rc = unsafe {
        RmRegisterResources(
            session.0,
            0,
            std::ptr::null(),
            explorers.len() as u32,
            explorers.as_ptr(),
            0,
            std::ptr::null(),
        )
    };
    if rc != 0 {
        return None;
    }
    let rc = unsafe { RmShutdown(session.0, RM_FORCE_SHUTDOWN, None) };
    if rc != 0 {
        // Partly shut down is possible: restart whatever did close.
        unsafe { RmRestart(session.0, 0, None) };
        return None;
    }
    Some(ExplorerPause { session })
}

// ---------------------------------------------------------------------------
// Services: stop and wait
// ---------------------------------------------------------------------------

use windows_sys::Win32::System::Services::{
    CloseServiceHandle, ControlService, OpenSCManagerW, OpenServiceW, QueryServiceStatusEx,
    SC_HANDLE, SERVICE_STATUS, SERVICE_STATUS_PROCESS,
};

const SC_MANAGER_CONNECT: u32 = 0x0001;
const SERVICE_QUERY_STATUS: u32 = 0x0004;
const SERVICE_STOP_ACCESS: u32 = 0x0020;
const SERVICE_CONTROL_STOP: u32 = 1;
const SC_STATUS_PROCESS_INFO: i32 = 0;
const STATE_STOPPED: u32 = 1;
const WIN32_OWN_PROCESS: u32 = 0x10;
const ERROR_SERVICE_DOES_NOT_EXIST: u32 = 1060;

struct ScHandle(SC_HANDLE);

impl ScHandle {
    fn new(h: SC_HANDLE) -> Option<Self> {
        (!h.is_null()).then_some(ScHandle(h))
    }
}

impl Drop for ScHandle {
    fn drop(&mut self) {
        unsafe { CloseServiceHandle(self.0) };
    }
}

fn service_status(svc: &ScHandle) -> Result<SERVICE_STATUS_PROCESS> {
    let mut st: SERVICE_STATUS_PROCESS = unsafe { std::mem::zeroed() };
    let mut needed = 0u32;
    let ok = unsafe {
        QueryServiceStatusEx(
            svc.0,
            SC_STATUS_PROCESS_INFO as _,
            &mut st as *mut _ as *mut u8,
            std::mem::size_of::<SERVICE_STATUS_PROCESS>() as u32,
            &mut needed,
        )
    };
    if ok == 0 {
        return Err(anyhow!(
            "query service status: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(st)
}

pub fn stop_service(name: &str, timeout: std::time::Duration) -> Result<crate::locks::ServiceStop> {
    use crate::locks::ServiceStop;
    use std::time::{Duration, Instant};
    let scm = ScHandle::new(unsafe {
        OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT)
    })
    .ok_or_else(|| anyhow!("open service manager: {}", std::io::Error::last_os_error()))?;
    let wname = wide(name);
    let Some(svc) = ScHandle::new(unsafe {
        OpenServiceW(
            scm.0,
            wname.as_ptr(),
            SERVICE_STOP_ACCESS | SERVICE_QUERY_STATUS,
        )
    }) else {
        let err = unsafe { GetLastError() };
        if err == ERROR_SERVICE_DOES_NOT_EXIST {
            return Ok(ServiceStop::NotFound);
        }
        return Err(anyhow!(
            "open service {name}: {}",
            std::io::Error::from_raw_os_error(err as i32)
        ));
    };
    let first = service_status(&svc)?;
    if first.dwCurrentState == STATE_STOPPED {
        return Ok(ServiceStop::WasNotRunning);
    }
    let pid = first.dwProcessId;
    let own_process = first.dwServiceType & WIN32_OWN_PROCESS != 0;
    let deadline = Instant::now() + timeout;
    let mut asked = false;
    loop {
        if !asked {
            let mut st: SERVICE_STATUS = unsafe { std::mem::zeroed() };
            // Refused while starting or already stopping: keep polling and
            // ask again, the service may accept it shortly.
            asked = unsafe { ControlService(svc.0, SERVICE_CONTROL_STOP, &mut st) } != 0;
        }
        let st = service_status(&svc)?;
        if st.dwCurrentState == STATE_STOPPED {
            break;
        }
        if Instant::now() >= deadline {
            anyhow::bail!("{name} did not stop within {} s", timeout.as_secs());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    // Stopped is reported before the process has finished exiting; its
    // open files are released only when it has.
    if own_process && pid != 0 {
        while process_alive(pid) {
            if Instant::now() >= deadline {
                anyhow::bail!(
                    "{name} stopped, but its process {pid} was still running after {} s",
                    timeout.as_secs()
                );
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    Ok(ServiceStop::Stopped)
}
