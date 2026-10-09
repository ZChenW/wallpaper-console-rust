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
    resolve_process_user_with(
        std::env::var("USER").ok(),
        std::env::var("LOGNAME").ok(),
        || unsafe { libc::getuid() },
        passwd_name_for_uid,
    )
}

fn resolve_process_user_with(
    user: Option<String>,
    logname: Option<String>,
    uid: impl FnOnce() -> u32,
    passwd_name: impl FnOnce(u32) -> Option<String>,
) -> ProcessUserScope {
    for user in [user, logname].into_iter().flatten() {
        let trimmed = user.trim();
        if !trimmed.is_empty() {
            return ProcessUserScope::Name(trimmed.to_string());
        }
    }

    let uid = uid();
    if let Some(name) = passwd_name(uid) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_env_is_preferred_over_passwd_and_uid() {
        let scope = resolve_process_user_with(
            Some("alice".to_string()),
            Some("bob".to_string()),
            || 7,
            |_| Some("carol".to_string()),
        );
        assert_eq!(scope, ProcessUserScope::Name("alice".to_string()));
    }

    #[test]
    fn empty_user_falls_through_to_logname_then_passwd() {
        let from_logname = resolve_process_user_with(
            Some("  ".to_string()),
            Some("bob".to_string()),
            || 7,
            |_| None,
        );
        assert_eq!(from_logname, ProcessUserScope::Name("bob".to_string()));

        let from_passwd =
            resolve_process_user_with(None, Some(String::new()), || 7, |_| Some("carol".into()));
        assert_eq!(from_passwd, ProcessUserScope::Name("carol".to_string()));
    }

    #[test]
    fn missing_passwd_entry_uses_numeric_uid_not_process_comm() {
        let comm = std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|status| {
                status
                    .lines()
                    .find_map(|line| line.strip_prefix("Name:\t"))
                    .map(|name| name.trim().to_string())
            })
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "wallpaper-conso".to_string());
        let scope = resolve_process_user_with(None, None, || 4242, |_| None);
        assert_eq!(scope, ProcessUserScope::Uid(4242));
        assert_ne!(scope, ProcessUserScope::Name(comm));
    }
}
