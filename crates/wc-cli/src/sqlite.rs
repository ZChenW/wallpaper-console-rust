use std::path::PathBuf;

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
        Commands::SqliteHistoryList => {
            for path in sqlite_list_table_paths(&s.cd, "history", "ORDER BY id DESC")? {
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

fn sqlite_connection(cd: &ConfigDir) -> anyhow::Result<rusqlite::Connection> {
    let db_path = cd.db_path();
    wc_storage::sqlite::ensure_sqlite_db(cd);
    rusqlite::Connection::open(&db_path)
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
        "sources" | "favorites" | "history" => {
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
