use crate::sqlite_err;
use rusqlite::{Connection, Transaction, TransactionBehavior};
use wc_core::config::ConfigDir;
use wc_core::error::WcError;

pub const LIBRARY_REVISION_KEY: &str = "library_revision";

pub fn read_library_revision(connection: &Connection) -> Result<u64, WcError> {
    let value = connection
        .query_row(
            "SELECT value FROM db_meta WHERE key = ?1",
            [LIBRARY_REVISION_KEY],
            |row| row.get::<_, String>(0),
        )
        .map_err(sqlite_err)?;
    value.parse::<u64>().map_err(|error| {
        WcError::Sqlite(format!(
            "invalid library revision stored in database: {error}"
        ))
    })
}

pub fn bump_library_revision(transaction: &Transaction<'_>) -> Result<u64, WcError> {
    transaction
        .execute(
            "UPDATE db_meta
             SET value = CAST(CAST(value AS INTEGER) + 1 AS TEXT),
                 updated_at = datetime('now')
             WHERE key = ?1",
            [LIBRARY_REVISION_KEY],
        )
        .map_err(sqlite_err)?;
    super::library_fts::mark_library_fts_stale_best_effort(transaction);
    read_library_revision(transaction)
}

pub fn read_library_change_state(connection: &Connection) -> Result<(i64, u64), WcError> {
    let data_version = connection
        .pragma_query_value(None, "data_version", |row| row.get::<_, i64>(0))
        .map_err(sqlite_err)?;
    Ok((data_version, read_library_revision(connection)?))
}

/// Hold SQLite's writer slot while publishing a revision-bound derived file.
/// The expensive file construction happens before this guard; the callback
/// should only atomically replace the file and update its dirty marker.
pub fn with_library_revision_publish_guard<T>(
    cd: &ConfigDir,
    expected_revision: u64,
    publish: impl FnOnce() -> Result<T, WcError>,
) -> Result<T, WcError> {
    let mut connection = super::open_runtime_connection(cd)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_err)?;
    let observed = read_library_revision(&transaction)?;
    if observed != expected_revision {
        return Err(WcError::RevisionChanged {
            expected: expected_revision,
            observed,
        });
    }
    let value = publish()?;
    transaction.commit().map_err(sqlite_err)?;
    Ok(value)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LibraryRevisionObserver {
    data_version: i64,
    library_revision: u64,
}

impl LibraryRevisionObserver {
    pub fn new(connection: &Connection) -> Result<Self, WcError> {
        let (data_version, library_revision) = read_library_change_state(connection)?;
        Ok(Self {
            data_version,
            library_revision,
        })
    }

    pub fn observe(&mut self, connection: &Connection) -> Result<Option<u64>, WcError> {
        let data_version = connection
            .pragma_query_value(None, "data_version", |row| row.get::<_, i64>(0))
            .map_err(sqlite_err)?;
        if data_version == self.data_version {
            return Ok(None);
        }
        self.data_version = data_version;
        let revision = read_library_revision(connection)?;
        if revision == self.library_revision {
            return Ok(None);
        }
        self.library_revision = revision;
        Ok(Some(revision))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observer_ignores_config_commits_and_reports_library_revision_commits() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("observer.db");
        let observer_connection = Connection::open(&path).unwrap();
        crate::sqlite::create_schema(&observer_connection).unwrap();
        let writer = Connection::open(&path).unwrap();
        let mut observer = LibraryRevisionObserver::new(&observer_connection).unwrap();

        writer
            .execute(
                "INSERT INTO config (key, value) VALUES ('theme', 'dark')",
                [],
            )
            .unwrap();
        assert_eq!(observer.observe(&observer_connection).unwrap(), None);

        let mut writer = writer;
        let tx = writer.transaction().unwrap();
        tx.execute("INSERT INTO favorites (path) VALUES ('/walls/a.jpg')", [])
            .unwrap();
        bump_library_revision(&tx).unwrap();
        tx.commit().unwrap();
        assert_eq!(observer.observe(&observer_connection).unwrap(), Some(1));
    }

    #[test]
    fn publish_guard_blocks_revision_writers_until_atomic_publish_finishes() {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        wc_config::ConfigDirExt::init(&cd).unwrap();
        crate::sqlite::try_ensure_sqlite_db(&cd).unwrap();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (finished_tx, finished_rx) = std::sync::mpsc::channel();
        let mut writer_thread = None;

        with_library_revision_publish_guard(&cd, 0, || {
            let writer_cd = ConfigDir {
                path: cd.path.clone(),
            };
            writer_thread = Some(std::thread::spawn(move || {
                let mut writer = Connection::open(writer_cd.db_path()).unwrap();
                writer
                    .busy_timeout(std::time::Duration::from_secs(2))
                    .unwrap();
                started_tx.send(()).unwrap();
                let transaction = writer.transaction().unwrap();
                bump_library_revision(&transaction).unwrap();
                transaction.commit().unwrap();
                finished_tx.send(()).unwrap();
            }));
            started_rx
                .recv_timeout(std::time::Duration::from_secs(1))
                .unwrap();
            assert!(finished_rx
                .recv_timeout(std::time::Duration::from_millis(100))
                .is_err());
            Ok(())
        })
        .unwrap();

        finished_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        writer_thread.take().unwrap().join().unwrap();
    }
}
