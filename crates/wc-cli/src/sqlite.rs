use std::path::PathBuf;

use wc_config::ConfigDirExt;
use wc_core::config::ConfigDir;
use wc_storage::StorageApi;

use crate::Commands;

pub(crate) fn run(cmd: Commands, s: &StorageApi) -> anyhow::Result<()> {
    match cmd {
        Commands::MigrateToSqlite => {
            unreachable!("MigrateToSqlite is handled before StorageApi::new");
        }
        Commands::SqliteVerify => match wc_storage::sqlite::verify(&s.cd) {
            Ok(wc_storage::sqlite::VerifyResult::Ok) => println!("VERIFY OK"),
            Ok(wc_storage::sqlite::VerifyResult::OkWithWarnings(warnings)) => {
                println!("VERIFY OK WITH WARNINGS");
                for w in &warnings {
                    println!("  warning: flat compatibility copy differs: {}", w);
                }
            }
            Ok(wc_storage::sqlite::VerifyResult::Failed(errors)) => {
                eprintln!(
                    "VERIFY FAILED: {} mismatch(es) found: {}",
                    errors.len(),
                    errors.join(", ")
                );
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("{}", e);
                let msg = e.to_string();
                if msg.contains("not found") {
                    std::process::exit(2);
                } else {
                    std::process::exit(1);
                }
            }
        },
        Commands::SqliteResync => {
            wc_storage::sqlite::repair(&s.cd)?;
            println!("Repair complete.");
        }
        Commands::SqliteExportFlat => {
            wc_storage::sqlite::export_flat(&s.cd)?;
            println!("Export complete.");
        }
        Commands::SqliteBackup => {
            let bak = wc_storage::sqlite::backup(&s.cd)?;
            println!("{}", bak);
        }
        Commands::SqliteRestore { backup } => {
            wc_storage::sqlite::restore(&s.cd, &PathBuf::from(&backup))?;
            println!("Restored.");
        }
        Commands::SqliteConfigGet { key } => {
            if let Some(value) = sqlite_config_get(&s.cd, &key)? {
                println!("{}", value);
            }
        }
        Commands::SqliteSourcesList => {
            for path in sqlite_list_table_paths(&s.cd, "sources", "ORDER BY path")? {
                println!("{}", path);
            }
        }
        Commands::SqliteFavoritesList => {
            for path in sqlite_list_table_paths(&s.cd, "favorites", "ORDER BY path")? {
                println!("{}", path);
            }
        }
        Commands::SqliteCurrentRead => {
            if let Some(value) = sqlite_state_get(&s.cd, "current")? {
                println!("{}", value);
            }
        }
        Commands::SqliteLastBackendRead => {
            if let Some(value) = sqlite_state_get(&s.cd, "last_backend")? {
                println!("{}", value);
            }
        }
        _ => unreachable!("sqlite::run called with non-sqlite command"),
    }
    Ok(())
}

pub(crate) fn migrate_to_sqlite() -> anyhow::Result<()> {
    let cd = ConfigDir::new()?;
    cd.init()?;
    let imported = wc_storage::sqlite::ensure_or_import_legacy_flat(&cd)?;
    if imported {
        println!(
            "Imported legacy flat files into: {}",
            cd.db_path().display()
        );
    } else {
        println!("SQLite already initialized at: {}", cd.db_path().display());
    }
    Ok(())
}

fn sqlite_connection(cd: &ConfigDir) -> anyhow::Result<wc_storage::sqlite::RuntimeConnection> {
    wc_storage::sqlite::try_ensure_sqlite_db(cd)?;
    wc_storage::sqlite::open_runtime_connection(cd)
        .map_err(|e| anyhow::anyhow!("failed to open wallpapers.db: {}", e))
}

fn sqlite_config_get(cd: &ConfigDir, key: &str) -> anyhow::Result<Option<String>> {
    let conn = sqlite_connection(cd)?;
    match conn.query_row(
        "SELECT value FROM config WHERE key=?1",
        rusqlite::params![key],
        |row| row.get(0),
    ) {
        Ok(value) => Ok(Some(value)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(anyhow::anyhow!("SQLite config read failed: {}", e)),
    }
}

fn sqlite_state_get(cd: &ConfigDir, key: &str) -> anyhow::Result<Option<String>> {
    let conn = sqlite_connection(cd)?;
    match conn.query_row(
        "SELECT value FROM state WHERE key=?1",
        rusqlite::params![key],
        |row| row.get(0),
    ) {
        Ok(value) => Ok(Some(value)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(anyhow::anyhow!("SQLite state read failed: {}", e)),
    }
}

fn sqlite_list_table_paths(
    cd: &ConfigDir,
    table: &str,
    order_clause: &str,
) -> anyhow::Result<Vec<String>> {
    let conn = sqlite_connection(cd)?;
    let sql = match table {
        "sources" | "favorites" => {
            format!("SELECT path FROM {} {}", table, order_clause)
        }
        _ => anyhow::bail!("unsupported SQLite path table: {}", table),
    };
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| anyhow::anyhow!("SQLite {} read failed: {}", table, e))?;
    let rows = stmt
        .query_map([], |row| row.get(0))
        .map_err(|e| anyhow::anyhow!("SQLite {} read failed: {}", table, e))?
        .collect::<Result<Vec<String>, _>>()
        .map_err(|e| anyhow::anyhow!("SQLite {} read failed: {}", table, e))?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_sqlite_helper_rejects_future_schema_without_changing_marker_or_data() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        let future_version = wc_storage::sqlite::CURRENT_SCHEMA_VERSION + 1;
        let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
        wc_storage::sqlite::create_schema(&conn).unwrap();
        conn.execute(
            "INSERT INTO config (key, value) VALUES ('sentinel', 'cli-value')",
            [],
        )
        .unwrap();
        conn.pragma_update(None, "user_version", future_version)
            .unwrap();
        drop(conn);

        let error = sqlite_config_get(&cd, "sentinel").err();

        let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
        let version = conn
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap();
        let sentinel: String = conn
            .query_row(
                "SELECT value FROM config WHERE key = 'sentinel'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let error = error.expect("CLI helper must reject a future-schema database");
        assert!(
            error.to_string().contains("newer") || error.to_string().contains("version"),
            "{error}"
        );
        assert_eq!(version, future_version);
        assert_eq!(sentinel, "cli-value");
    }
}
