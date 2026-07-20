use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use wc_config::ConfigDirExt;
use wc_core::error::WcError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WeCompatEntry {
    pub project_path: String,
    pub backend_status: String,
    pub error_kind: String,
    pub error_message: String,
    pub error_detail: Option<String>,
    pub failed_at: String,
    pub project_json_mtime: Option<u64>,
}

#[derive(Debug)]
pub struct WeCompatCache {
    map: HashMap<String, WeCompatEntry>,
}

static ATOMIC_WE_COMPAT_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn cache_path() -> Result<PathBuf, WcError> {
    let cd = wc_core::ConfigDir::new()?;
    cd.init()?;
    Ok(cd.path.join("we_compatibility.json"))
}

fn atomic_we_compat_temp_path(path: &Path) -> PathBuf {
    let sequence = ATOMIC_WE_COMPAT_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let mut temp_file_name = OsString::from(".");
    temp_file_name.push(
        path.file_name()
            .unwrap_or_else(|| OsStr::new("we_compatibility.json")),
    );
    temp_file_name.push(format!(".tmp.{}.{}", std::process::id(), sequence));
    path.with_file_name(temp_file_name)
}

fn now_secs() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

fn project_json_mtime(project_path: &str) -> Option<u64> {
    std::fs::metadata(Path::new(project_path).join("project.json"))
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
}

fn load_map() -> Result<HashMap<String, WeCompatEntry>, WcError> {
    let path = cache_path()?;
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let text = std::fs::read_to_string(path).map_err(WcError::Io)?;
    Ok(serde_json::from_str(&text).unwrap_or_default())
}

fn save(map: &HashMap<String, WeCompatEntry>) -> Result<(), WcError> {
    let path = cache_path()?;
    let text = serde_json::to_string_pretty(map).map_err(|e| WcError::Other(e.to_string()))?;
    let tmp = atomic_we_compat_temp_path(&path);
    if let Err(error) = std::fs::write(&tmp, text) {
        let _ = std::fs::remove_file(&tmp);
        return Err(WcError::Io(error));
    }
    if let Err(error) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(WcError::Io(error));
    }
    Ok(())
}

impl WeCompatCache {
    pub fn load() -> Result<Self, WcError> {
        Ok(Self {
            map: load_map()?,
        })
    }

    pub fn lookup_failure(&mut self, project_path: &str) -> Result<Option<WeCompatEntry>, WcError> {
        let Some(entry) = self.map.get(project_path).cloned() else {
            return Ok(None);
        };
        if entry.project_json_mtime != project_json_mtime(project_path) {
            self.map.remove(project_path);
            save(&self.map).ok();
            return Ok(None);
        }
        Ok(Some(entry))
    }
}

pub fn record_failure(
    project_path: &str,
    backend_status: &str,
    error_kind: &str,
    error_message: &str,
    error_detail: Option<String>,
) -> Result<(), WcError> {
    let mut map = load_map()?;
    map.insert(
        project_path.to_string(),
        WeCompatEntry {
            project_path: project_path.to_string(),
            backend_status: backend_status.into(),
            error_kind: error_kind.into(),
            error_message: error_message.into(),
            error_detail,
            failed_at: now_secs(),
            project_json_mtime: project_json_mtime(project_path),
        },
    );
    save(&map)
}

pub fn lookup_failure(project_path: &str) -> Result<Option<WeCompatEntry>, WcError> {
    let mut cache = WeCompatCache::load()?;
    cache.lookup_failure(project_path)
}

pub fn clear_failure(project_path: &str) -> Result<(), WcError> {
    let mut map = load_map()?;
    map.remove(project_path);
    save(&map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn record_lookup_and_clear_failure() {
        let _guard = ENV_LOCK.lock().unwrap();
        let previous = std::env::var_os("XDG_CONFIG_HOME");
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_CONFIG_HOME", tmp.path());
        let project = tmp.path().join("p");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("project.json"), "{}").unwrap();
        record_failure(
            &project.to_string_lossy(),
            "failed",
            "kind",
            "message",
            Some("detail".into()),
        )
        .unwrap();
        let found = lookup_failure(&project.to_string_lossy()).unwrap().unwrap();
        assert_eq!(found.error_kind, "kind");
        clear_failure(&project.to_string_lossy()).unwrap();
        assert!(lookup_failure(&project.to_string_lossy())
            .unwrap()
            .is_none());
        match previous {
            Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }

    #[test]
    fn cache_load_reuses_single_file_read() {
        let _guard = ENV_LOCK.lock().unwrap();
        let previous = std::env::var_os("XDG_CONFIG_HOME");
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_CONFIG_HOME", tmp.path());
        let project = tmp.path().join("p");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("project.json"), "{}").unwrap();
        record_failure(
            &project.to_string_lossy(),
            "failed",
            "kind",
            "message",
            None,
        )
        .unwrap();

        let mut cache = WeCompatCache::load().unwrap();
        assert!(cache
            .lookup_failure(&project.to_string_lossy())
            .unwrap()
            .is_some());
        assert!(cache
            .lookup_failure(&project.to_string_lossy())
            .unwrap()
            .is_some());

        match previous {
            Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }

    #[test]
    fn save_writes_via_temp_file_rename() {
        let _guard = ENV_LOCK.lock().unwrap();
        let previous = std::env::var_os("XDG_CONFIG_HOME");
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_CONFIG_HOME", tmp.path());
        let project = tmp.path().join("p");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("project.json"), "{}").unwrap();

        record_failure(
            &project.to_string_lossy(),
            "failed",
            "kind",
            "message",
            None,
        )
        .unwrap();

        let path = cache_path().unwrap();
        assert!(path.exists());
        assert!(
            tmp.path()
                .read_dir()
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .all(|entry| !entry
                    .file_name()
                    .and_then(OsStr::to_str)
                    .is_some_and(|name| name.contains(".tmp."))),
            "compat cache should not leave temp files behind"
        );

        match previous {
            Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }
}
