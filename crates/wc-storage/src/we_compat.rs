use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
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

fn cache_path() -> Result<PathBuf, WcError> {
    let cd = wc_core::ConfigDir::new()?;
    cd.init()?;
    Ok(cd.path.join("we_compatibility.json"))
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

fn load() -> Result<HashMap<String, WeCompatEntry>, WcError> {
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
    std::fs::write(path, text).map_err(WcError::Io)
}

pub fn record_failure(
    project_path: &str,
    backend_status: &str,
    error_kind: &str,
    error_message: &str,
    error_detail: Option<String>,
) -> Result<(), WcError> {
    let mut map = load()?;
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
    let mut map = load()?;
    let Some(entry) = map.get(project_path).cloned() else {
        return Ok(None);
    };
    if entry.project_json_mtime != project_json_mtime(project_path) {
        map.remove(project_path);
        save(&map).ok();
        return Ok(None);
    }
    Ok(Some(entry))
}

pub fn clear_failure(project_path: &str) -> Result<(), WcError> {
    let mut map = load()?;
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
}
