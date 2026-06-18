use std::path::PathBuf;

use super::common::{fail, ok, storage, CommandResult};

#[tauri::command]
pub async fn migrate_to_sqlite() -> CommandResult {
    import_legacy_flat_files().await
}

#[tauri::command]
pub async fn import_legacy_flat_files() -> CommandResult {
    tauri::async_runtime::spawn_blocking(|| {
        let cd = match wc_core::ConfigDir::new() {
            Ok(c) => c,
            Err(e) => return fail(e.to_string()),
        };
        if let Err(e) = cd.init() {
            return fail(e.to_string());
        }
        match wc_storage::sqlite::ensure_or_import_legacy_flat(&cd) {
            Ok(true) => ok("Imported legacy flat files into SQLite."),
            Ok(false) => ok("SQLite already initialized."),
            Err(e) => fail(e.to_string()),
        }
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn sqlite_verify() -> CommandResult {
    tauri::async_runtime::spawn_blocking(|| match storage() {
        Ok(s) => match wc_storage::sqlite::verify(&s.cd) {
            Ok(wc_storage::sqlite::VerifyResult::Ok) => ok("VERIFY OK"),
            Ok(wc_storage::sqlite::VerifyResult::OkWithWarnings(warnings)) => {
                ok(format!("VERIFY OK WITH WARNINGS\n{}", warnings.join("\n")))
            }
            Ok(wc_storage::sqlite::VerifyResult::Failed(errors)) => fail(format!(
                "VERIFY FAILED: {} mismatch(es) found: {}",
                errors.len(),
                errors.join(", ")
            )),
            Err(e) => fail(e.to_string()),
        },
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn sqlite_repair() -> CommandResult {
    tauri::async_runtime::spawn_blocking(|| match storage() {
        Ok(s) => match wc_storage::sqlite::repair(&s.cd) {
            Ok(()) => ok("Repair complete."),
            Err(e) => fail(e.to_string()),
        },
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn sqlite_resync() -> CommandResult {
    sqlite_repair().await
}

#[tauri::command]
pub async fn sqlite_backup() -> CommandResult {
    tauri::async_runtime::spawn_blocking(|| match storage() {
        Ok(s) => match wc_storage::sqlite::backup(&s.cd) {
            Ok(path) => ok(path),
            Err(e) => fail(e.to_string()),
        },
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn sqlite_restore(path: String) -> CommandResult {
    tauri::async_runtime::spawn_blocking(move || match storage() {
        Ok(s) => match wc_storage::sqlite::restore(&s.cd, &PathBuf::from(path)) {
            Ok(()) => ok("Restore complete."),
            Err(e) => fail(e.to_string()),
        },
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn sqlite_export_flat() -> CommandResult {
    tauri::async_runtime::spawn_blocking(|| match storage() {
        Ok(s) => match wc_storage::sqlite::export_flat(&s.cd) {
            Ok(()) => ok("Export complete."),
            Err(e) => fail(e.to_string()),
        },
        Err(e) => fail(e),
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}
