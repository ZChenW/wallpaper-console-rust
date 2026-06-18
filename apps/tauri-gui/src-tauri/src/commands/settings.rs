use std::collections::HashMap;

use super::common::{fail, ok, storage, CommandResult};

#[tauri::command]
pub async fn config_get(key: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let s = storage()?;
        Ok(s.config_get(&key, ""))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn config_get_many(keys: Vec<String>) -> Result<HashMap<String, String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let s = storage()?;
        let mut out = HashMap::new();
        for key in keys {
            out.insert(key.clone(), s.config_get(&key, ""));
        }
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn config_set(key: String, value: String) -> CommandResult {
    tauri::async_runtime::spawn_blocking(move || match storage() {
        Ok(s) => match s.config_set(&key, &value) {
            Ok(()) => ok(format!("{} = {}", key, value)),
            Err(e) => fail(e.to_string()),
        },
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn export_diagnostics() -> CommandResult {
    tauri::async_runtime::spawn_blocking(|| match storage() {
        Ok(s) => {
            let dir = s.cd.path.join("diagnostics");
            if let Err(e) = std::fs::create_dir_all(&dir) {
                return fail(e.to_string());
            }
            let path = dir.join(format!(
                "wallpaper-console-diagnostics-{}.txt",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            ));
            let current = s.current_read().unwrap_or_default().unwrap_or_default();
            let content = format!(
                "wallpaper-console diagnostics\nconfig_dir={}\ncurrent={}\nsources={}\n",
                s.cd.path
                    .file_name()
                    .map(|n| n.to_string_lossy())
                    .unwrap_or_default(),
                std::path::Path::new(&current)
                    .file_name()
                    .map(|n| n.to_string_lossy())
                    .unwrap_or_default(),
                s.sources_list().unwrap_or_default().len()
            );
            match std::fs::write(&path, content) {
                Ok(()) => ok(path.to_string_lossy().to_string()),
                Err(e) => fail(e.to_string()),
            }
        }
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}
