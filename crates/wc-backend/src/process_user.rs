//! Cached user scope for renderer process discovery and control.

#[cfg(test)]
pub(crate) fn whoami() -> String {
    match current_process_user() {
        ProcessUserScope::Name(name) => name,
        ProcessUserScope::Uid(uid) => uid.to_string(),
    }
}

/// Scope for pgrep/pkill user filtering. Prefer login name when available;
/// fall back to numeric uid so process queries still work without USER set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ProcessUserScope {
    Name(String),
    Uid(u32),
}

pub(crate) fn current_process_user() -> ProcessUserScope {
    static RESOLVED: std::sync::OnceLock<ProcessUserScope> = std::sync::OnceLock::new();
    RESOLVED.get_or_init(resolve_process_user).clone()
}

pub(crate) fn append_pgrep_user_scope(cmd: &mut std::process::Command, scope: &ProcessUserScope) {
    match scope {
        ProcessUserScope::Name(name) => {
            cmd.arg("-u").arg(name);
        }
        ProcessUserScope::Uid(uid) => {
            cmd.arg("-U").arg(uid.to_string());
        }
    }
}

fn resolve_process_user() -> ProcessUserScope {
    for key in ["USER", "LOGNAME"] {
        if let Ok(user) = std::env::var(key) {
            let trimmed = user.trim();
            if !trimmed.is_empty() {
                return ProcessUserScope::Name(trimmed.to_string());
            }
        }
    }

    let uid = unsafe { libc::getuid() };
    if let Some(name) = passwd_name_for_uid(uid) {
        return ProcessUserScope::Name(name);
    }
    if let Some(name) = passwd_name_from_proc_status() {
        return ProcessUserScope::Name(name);
    }

    log::error!(
        "wc-backend: could not resolve login name for uid {uid}; using uid scope for process queries"
    );
    ProcessUserScope::Uid(uid)
}

fn passwd_name_for_uid(uid: u32) -> Option<String> {
    use std::ffi::CStr;
    use std::ptr;

    let mut buf = vec![0u8; 16_384];
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::passwd = ptr::null_mut();
    let rc = unsafe {
        libc::getpwuid_r(
            uid as libc::uid_t,
            &mut pwd,
            buf.as_mut_ptr() as *mut libc::c_char,
            buf.len(),
            &mut result,
        )
    };
    if rc != 0 || result.is_null() {
        return None;
    }
    unsafe {
        if pwd.pw_name.is_null() {
            return None;
        }
        CStr::from_ptr(pwd.pw_name)
            .to_str()
            .ok()
            .map(|name| name.to_owned())
    }
}

fn passwd_name_from_proc_status() -> Option<String> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(name) = line.strip_prefix("Name:\t") {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}
