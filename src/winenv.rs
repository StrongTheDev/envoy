//! Everything that talks to Windows: the registry, elevation and the
//! "environment changed" broadcast.

use crate::model::{Change, ChangeOp, EnvSet, Scope, Var};
use std::borrow::Cow;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use winreg::enums::*;
use winreg::{RegKey, RegValue};

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_CANCELLED, HANDLE};
use windows_sys::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetExitCodeProcess, OpenProcessToken, WaitForSingleObject, INFINITE,
};
use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, SW_SHOWNORMAL, WM_SETTINGCHANGE,
};

const USER_KEY: &str = "Environment";
const SYSTEM_KEY: &str = r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment";

fn open(scope: Scope, perms: u32) -> io::Result<RegKey> {
    match scope {
        Scope::User => RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(USER_KEY, perms),
        Scope::System => RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(SYSTEM_KEY, perms),
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn decode_utf16(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
    String::from_utf16_lossy(&units[..end])
}

pub fn read_scope(scope: Scope) -> io::Result<Vec<Var>> {
    let key = open(scope, KEY_READ)?;
    let mut vars = Vec::new();
    for item in key.enum_values() {
        let (name, val) = item?;
        let expand = match val.vtype {
            REG_SZ => false,
            REG_EXPAND_SZ => true,
            _ => continue, // not a string: leave it alone
        };
        if name.is_empty() {
            continue;
        }
        vars.push(Var { name, value: decode_utf16(&val.bytes), expand });
    }
    vars.sort_by_key(|v| v.name.to_lowercase());
    Ok(vars)
}

pub fn read_all() -> io::Result<EnvSet> {
    Ok(EnvSet { user: read_scope(Scope::User)?, system: read_scope(Scope::System)? })
}

/// Write a set of changes. Stops at the first failure.
pub fn apply_changes(changes: &[Change]) -> Result<(), String> {
    for scope in Scope::ALL {
        let mine: Vec<&Change> = changes.iter().filter(|c| c.scope == scope).collect();
        if mine.is_empty() {
            continue;
        }
        let key = open(scope, KEY_SET_VALUE | KEY_QUERY_VALUE)
            .map_err(|e| format!("Cannot open the {} environment for writing: {e}", scope.lower()))?;
        // Deletes first so a case-only rename ends up with the new spelling.
        for c in mine.iter().filter(|c| c.op == ChangeOp::Delete) {
            match key.delete_value(&c.name) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("Cannot delete {}: {e}", c.name)),
            }
        }
        for c in &mine {
            if let ChangeOp::Set { value, expand } = &c.op {
                let bytes: Vec<u8> = wide(value).iter().flat_map(|u| u.to_le_bytes()).collect();
                let val = RegValue {
                    bytes: Cow::Owned(bytes),
                    vtype: if *expand { REG_EXPAND_SZ } else { REG_SZ },
                };
                key.set_raw_value(&c.name, &val)
                    .map_err(|e| format!("Cannot write {}: {e}", c.name))?;
            }
        }
    }
    Ok(())
}

/// Tell running applications (Explorer, shells that listen) to reload the environment.
/// Can take a few seconds if some window is not responding, so call it off the UI thread.
pub fn broadcast_change() {
    let param = wide("Environment");
    let mut result: usize = 0;
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            param.as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            3000,
            &mut result,
        );
    }
}

pub fn is_elevated() -> bool {
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            &mut elevation as *mut _ as *mut _,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
}

pub enum ElevateError {
    /// The UAC prompt was dismissed.
    Cancelled,
    Other(String),
}

/// Start this executable again through UAC. With `wait`, blocks until it exits
/// and returns its exit code.
pub fn run_self_elevated(args: &[&str], wait: bool) -> Result<Option<u32>, ElevateError> {
    let exe = std::env::current_exe().map_err(|e| ElevateError::Other(e.to_string()))?;
    let exe_w: Vec<u16> = exe.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let params = args.iter().map(|a| quote_arg(a)).collect::<Vec<_>>().join(" ");
    let params_w = wide(&params);
    let verb = wide("runas");

    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS;
    info.lpVerb = verb.as_ptr();
    info.lpFile = exe_w.as_ptr();
    info.lpParameters = params_w.as_ptr();
    info.nShow = SW_SHOWNORMAL as i32;

    unsafe {
        if ShellExecuteExW(&mut info) == 0 {
            let code = GetLastError();
            return Err(if code == ERROR_CANCELLED {
                ElevateError::Cancelled
            } else {
                ElevateError::Other(io::Error::from_raw_os_error(code as i32).to_string())
            });
        }
        if info.hProcess.is_null() {
            return Ok(None);
        }
        let mut exit = None;
        if wait {
            WaitForSingleObject(info.hProcess, INFINITE);
            let mut code = 0u32;
            if GetExitCodeProcess(info.hProcess, &mut code) != 0 {
                exit = Some(code);
            }
        }
        CloseHandle(info.hProcess);
        Ok(exit)
    }
}

fn quote_arg(a: &str) -> String {
    if !a.is_empty() && !a.contains([' ', '\t', '"']) {
        return a.to_owned();
    }
    // Backslashes only need doubling when they precede a quote.
    let mut out = String::from("\"");
    let mut slashes = 0;
    for ch in a.chars() {
        match ch {
            '\\' => slashes += 1,
            '"' => {
                out.extend(std::iter::repeat('\\').take(slashes * 2 + 1));
                slashes = 0;
                out.push('"');
                continue;
            }
            _ => {
                out.extend(std::iter::repeat('\\').take(slashes));
                slashes = 0;
            }
        }
        if ch != '\\' {
            out.push(ch);
        }
    }
    out.extend(std::iter::repeat('\\').take(slashes * 2));
    out.push('"');
    out
}

/// Entry point of the elevated helper: apply the changes stored in `file`.
/// Errors are written next to it as `<file>.err`.
pub fn elevated_apply_main(file: &Path) -> i32 {
    let result = std::fs::read(file)
        .map_err(|e| e.to_string())
        .and_then(|b| serde_json::from_slice::<Vec<Change>>(&b).map_err(|e| e.to_string()))
        .and_then(|changes| apply_changes(&changes));
    match result {
        Ok(()) => 0,
        Err(e) => {
            let _ = std::fs::write(err_file(file), e);
            1
        }
    }
}

pub fn err_file(file: &Path) -> std::path::PathBuf {
    let mut s = file.as_os_str().to_owned();
    s.push(".err");
    s.into()
}

pub fn user_name() -> String {
    std::env::var("USERNAME").unwrap_or_else(|_| "user".into())
}

/// "Windows 11", "Windows 10", …
pub fn os_label() -> String {
    let key = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion", KEY_READ);
    let Ok(key) = key else { return "Windows".into() };
    let build: u32 = key
        .get_value::<String, _>("CurrentBuildNumber")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    if build >= 22000 {
        "Windows 11".into()
    } else if build >= 10240 {
        "Windows 10".into()
    } else {
        key.get_value::<String, _>("ProductName").unwrap_or_else(|_| "Windows".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting() {
        assert_eq!(quote_arg("--apply"), "--apply");
        assert_eq!(quote_arg(r"C:\a b\c.json"), r#""C:\a b\c.json""#);
        assert_eq!(quote_arg(r"C:\a b\"), r#""C:\a b\\""#);
    }

    /// Writes (and removes) a throwaway user variable in the real registry.
    /// Run on purpose with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn writes_and_deletes_a_user_variable() {
        let name = "ENVOY_SELFTEST_TMP";
        let set = |name: &str, value: &str, expand: bool| Change {
            scope: Scope::User,
            name: name.into(),
            op: ChangeOp::Set { value: value.into(), expand },
        };
        let del = |name: &str| Change { scope: Scope::User, name: name.into(), op: ChangeOp::Delete };
        let find = || read_scope(Scope::User).unwrap().into_iter().find(|v| v.name.eq_ignore_ascii_case(name));

        apply_changes(&[set(name, "plain ünï", false)]).unwrap();
        assert_eq!(find().unwrap(), Var { name: name.into(), value: "plain ünï".into(), expand: false });

        let long = format!(r"%USERPROFILE%\bin;{}", r"C:\some\folder;".repeat(300));
        assert!(long.len() > 2047);
        apply_changes(&[set(name, &long, true)]).unwrap();
        let got = find().unwrap();
        assert!(got.expand && got.value == long);

        // Case-only rename: delete + set in one batch must leave the new spelling.
        apply_changes(&[del(name), set("Envoy_SelfTest_Tmp", "x", false)]).unwrap();
        assert_eq!(find().unwrap().name, "Envoy_SelfTest_Tmp");

        apply_changes(&[del("Envoy_SelfTest_Tmp")]).unwrap();
        assert!(find().is_none());
        apply_changes(&[del(name)]).unwrap(); // deleting something absent is fine
    }

    #[test]
    fn reads_the_real_environment() {

        let env = read_all().expect("registry readable");
        assert!(env.system.iter().any(|v| v.name.eq_ignore_ascii_case("Path")));
    }
}
