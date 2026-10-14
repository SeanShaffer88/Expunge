//! Windows implementations: registry access, process launching, elevation.

use super::Registry;
use crate::model::{fold, KeyData, RegValue};
use anyhow::{anyhow, Context, Result};
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use winreg::enums::*;
use winreg::RegKey;

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
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
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
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
            let rv = RegValue { name: name.clone(), kind: v.vtype.clone() as u32, data: v.bytes.to_vec() };
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
            return Err(anyhow!("RegSetValueEx {key}\\{} failed: {}", value.name, std::io::Error::from_raw_os_error(rc as i32)));
        }
        Ok(())
    }

    fn delete_value(&self, key: &str, name: &str) -> Result<()> {
        let k = open(key, KEY_SET_VALUE).with_context(|| format!("open {key}"))?;
        k.delete_value(name).with_context(|| format!("delete value {key}\\{name}"))?;
        Ok(())
    }

    fn delete_tree(&self, path: &str) -> Result<()> {
        let (h, rest) = super::split_hive(path).ok_or_else(|| anyhow!("unknown hive: {path}"))?;
        if rest.is_empty() {
            return Err(anyhow!("refusing to delete a hive root"));
        }
        let (parent, leaf) = match rest.rsplit_once('\\') {
            Some((p, l)) => (format!("{h}\\{p}"), l.to_string()),
            None => (h.to_string(), rest.to_string()),
        };
        let pk = open(&parent, KEY_ALL_ACCESS).with_context(|| format!("open {parent}"))?;
        pk.delete_subkey_all(&leaf).with_context(|| format!("delete {path}"))?;
        Ok(())
    }
}

pub fn delete_on_reboot(path: &Path) -> Result<()> {
    let p = wide(&path.to_string_lossy());
    let ok = unsafe { MoveFileExW(p.as_ptr(), std::ptr::null(), MOVEFILE_DELAY_UNTIL_REBOOT) };
    if ok == 0 {
        return Err(anyhow!("MoveFileEx failed: {}", std::io::Error::last_os_error()));
    }
    Ok(())
}

pub fn is_elevated() -> bool {
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elev = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            &mut elev as *mut _ as *mut _,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        CloseHandle(token);
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
            if WaitForSingleObject(self.handle, 0) == WAIT_OBJECT_0 {
                let mut code = 0u32;
                GetExitCodeProcess(self.handle, &mut code);
                Some(code)
            } else {
                None
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
            return Ok(ChildProcess { handle: pi.hProcess });
        }
        let err = GetLastError();
        if err == ERROR_ELEVATION_REQUIRED {
            let (prog, args) = crate::util::split_command_line(cmdline);
            return shell_execute_wait(&prog, &args, true);
        }
        Err(anyhow!("could not start `{cmdline}`: {}", std::io::Error::from_raw_os_error(err as i32)))
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
        info.lpParameters = if params.is_empty() { std::ptr::null() } else { p.as_ptr() };
        info.nShow = SW_SHOWNORMAL;
        if ShellExecuteExW(&mut info) == 0 {
            return Err(anyhow!("could not start {file}: {}", std::io::Error::last_os_error()));
        }
        Ok(ChildProcess { handle: info.hProcess })
    }
}

/// Open a file, folder or URL with its default handler.
pub fn shell_open(target: &str) {
    let verb = wide("open");
    let t = wide(target);
    unsafe {
        ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), t.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL);
    }
}

/// Restart Expunge elevated. Returns true if the new process launched.
pub fn relaunch_elevated() -> bool {
    let exe = std::env::current_exe().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    shell_execute_wait(&exe, "", true).is_ok()
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
    let long = super::HIVES.iter().find(|(s, _)| *s == h).map(|(_, l)| *l).unwrap_or(h);
    let target = if rest.is_empty() { format!("Computer\\{long}") } else { format!("Computer\\{long}\\{rest}") };
    let applet = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Applets\\Regedit")?
        .0;
    applet.set_value("LastKey", &target)?;
    // Regedit is single-instance; /m opens a new window that honours LastKey.
    shell_execute_wait("regedit.exe", "/m", false)?;
    Ok(())
}
