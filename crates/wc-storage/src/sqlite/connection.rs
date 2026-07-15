use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use crate::sqlite_err;
use fs2::FileExt;
use rusqlite::Connection;
use std::fs::{File, OpenOptions};
#[cfg(test)]
use wc_config::ConfigDirExt;
use wc_core::config::ConfigDir;
use wc_core::error::WcError;

pub const RUNTIME_BUSY_TIMEOUT_MS: u64 = 5000;

fn apply_busy_timeout(connection: &Connection) -> Result<(), WcError> {
    connection
        .busy_timeout(Duration::from_millis(RUNTIME_BUSY_TIMEOUT_MS))
        .map_err(sqlite_err)
}

/// Apply the connection-local invariants required by all runtime queries.
pub fn apply_runtime_pragmas(connection: &Connection) -> Result<(), WcError> {
    apply_busy_timeout(connection)?;
    connection
        .execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;",
        )
        .map_err(sqlite_err)?;
    Ok(())
}

/// Process-wide generation bump used to discard leased/cached runtime
/// connections before exclusive maintenance or schema work.
static CACHE_GENERATION: AtomicU64 = AtomicU64::new(0);
static RUNTIME_CONNECTION_CACHE: Mutex<Option<CachedRuntimeConnection>> = Mutex::new(None);

struct CachedRuntimeConnection {
    generation: u64,
    db_path: PathBuf,
    supported_schema_version: i64,
    /// Bootstrap/migrate connections hold an exclusive schema lock and must
    /// never re-enter the idle cache.
    cacheable: bool,
    connection: Connection,
    /// Held for the connection lifetime so DB replacement stays blocked.
    _schema_guard: MaintenanceGuard,
    _maintenance_guard: MaintenanceGuard,
}

/// Drop any idle cached runtime connection so its shared file locks release.
///
/// Active leases keep their locks until they drop; a generation bump prevents
/// those leases from returning a stale connection to the cache afterward.
/// Exclusive maintenance / schema lock acquisition calls this first so the
/// same process does not deadlock itself on a cached shared lock.
pub fn invalidate_cached_connections() {
    CACHE_GENERATION.fetch_add(1, Ordering::SeqCst);
    let mut cache = RUNTIME_CONNECTION_CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *cache = None;
}

fn lock_runtime_cache() -> std::sync::MutexGuard<'static, Option<CachedRuntimeConnection>> {
    RUNTIME_CONNECTION_CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub struct RuntimeConnection {
    cached: Option<CachedRuntimeConnection>,
}

impl Deref for RuntimeConnection {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        &self
            .cached
            .as_ref()
            .expect("runtime SQLite connection is available until drop")
            .connection
    }
}

impl DerefMut for RuntimeConnection {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self
            .cached
            .as_mut()
            .expect("runtime SQLite connection is available until drop")
            .connection
    }
}

impl Drop for RuntimeConnection {
    fn drop(&mut self) {
        let Some(entry) = self.cached.take() else {
            return;
        };
        if !entry.cacheable {
            return;
        }
        let current = CACHE_GENERATION.load(Ordering::SeqCst);
        if entry.generation != current {
            // Invalidated while leased: drop connection + unlock.
            return;
        }
        let mut cache = lock_runtime_cache();
        if cache.is_none() {
            *cache = Some(entry);
        }
        // Otherwise another live connection already occupies the slot; drop.
    }
}

pub(super) struct MaintenanceGuard {
    file: File,
}

impl Drop for MaintenanceGuard {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

fn maintenance_lock_path(cd: &ConfigDir) -> PathBuf {
    cd.path.join(".wallpapers.db.maintenance.lock")
}

fn schema_lock_path(cd: &ConfigDir) -> PathBuf {
    cd.path.join(".wallpapers.db.schema.lock")
}

fn open_lock(cd: &ConfigDir, path: PathBuf) -> Result<File, WcError> {
    std::fs::create_dir_all(&cd.path).map_err(WcError::Io)?;
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(WcError::Io)
}

fn acquire_shared_lock(cd: &ConfigDir) -> Result<MaintenanceGuard, WcError> {
    let file = open_lock(cd, maintenance_lock_path(cd))?;
    FileExt::lock_shared(&file).map_err(WcError::Io)?;
    Ok(MaintenanceGuard { file })
}

pub(super) fn acquire_maintenance_lock(cd: &ConfigDir) -> Result<MaintenanceGuard, WcError> {
    // Release any idle cached shared locks before blocking for exclusive.
    invalidate_cached_connections();
    let file = open_lock(cd, maintenance_lock_path(cd))?;
    FileExt::lock_exclusive(&file).map_err(WcError::Io)?;
    Ok(MaintenanceGuard { file })
}

#[cfg(test)]
pub(crate) fn take_exclusive_maintenance_lock(cd: &ConfigDir) -> Result<(), WcError> {
    let _guard = acquire_maintenance_lock(cd)?;
    Ok(())
}

fn acquire_schema_lock(cd: &ConfigDir) -> Result<MaintenanceGuard, WcError> {
    invalidate_cached_connections();
    let file = open_lock(cd, schema_lock_path(cd))?;
    FileExt::lock_exclusive(&file).map_err(WcError::Io)?;
    Ok(MaintenanceGuard { file })
}

fn acquire_schema_shared_lock(cd: &ConfigDir) -> Result<MaintenanceGuard, WcError> {
    let file = open_lock(cd, schema_lock_path(cd))?;
    FileExt::lock_shared(&file).map_err(WcError::Io)?;
    Ok(MaintenanceGuard { file })
}

fn runtime_connection(cached: CachedRuntimeConnection) -> RuntimeConnection {
    RuntimeConnection {
        cached: Some(cached),
    }
}

/// Open or create the main database while holding the normal shared lock.
///
/// Schema bootstrapping deliberately applies its PRAGMAs after checking the
/// database version, so this helper only opens the connection.
pub(super) fn open_or_create_connection(cd: &ConfigDir) -> Result<RuntimeConnection, WcError> {
    let guard = acquire_shared_lock(cd)?;
    // Multiple GUI/CLI startups may bootstrap the same schema concurrently.
    // Serialize that write phase without weakening the shared maintenance lock
    // that prevents a database replacement after this connection opens.
    let schema_guard = acquire_schema_lock(cd)?;
    let connection = Connection::open(cd.db_path()).map_err(sqlite_err)?;
    // Schema initialization can write. Apply the wait policy before callers
    // inspect/migrate the schema, without changing a future-version database.
    apply_busy_timeout(&connection)?;
    Ok(runtime_connection(CachedRuntimeConnection {
        generation: CACHE_GENERATION.load(Ordering::SeqCst),
        db_path: cd.db_path(),
        supported_schema_version: i64::MIN,
        cacheable: false,
        connection,
        _schema_guard: schema_guard,
        _maintenance_guard: guard,
    }))
}

pub(super) fn open_runtime_connection(
    cd: &ConfigDir,
    supported_schema_version: i64,
) -> Result<RuntimeConnection, WcError> {
    let db_path = cd.db_path();
    if !db_path.exists() {
        return Err(WcError::Sqlite(format!(
            "database not found: {}",
            db_path.display()
        )));
    }

    {
        let mut cache = lock_runtime_cache();
        if let Some(entry) = cache.take() {
            let current = CACHE_GENERATION.load(Ordering::SeqCst);
            if entry.generation == current
                && entry.db_path == db_path
                && entry.supported_schema_version == supported_schema_version
            {
                return Ok(runtime_connection(entry));
            }
            // Path/version/generation mismatch: drop and open fresh below.
        }
    }

    let guard = acquire_shared_lock(cd)?;
    let schema_guard = acquire_schema_shared_lock(cd)?;
    // Re-check existence after waiting for locks: a maintenance replace may
    // have removed/replaced the file while we were blocked.
    if !db_path.exists() {
        return Err(WcError::Sqlite(format!(
            "database not found: {}",
            db_path.display()
        )));
    }
    let connection = Connection::open(&db_path).map_err(sqlite_err)?;
    apply_busy_timeout(&connection)?;
    let schema_version = connection
        .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
        .map_err(sqlite_err)?;
    if schema_version > supported_schema_version {
        return Err(WcError::Sqlite(format!(
            "database schema version {schema_version} is newer than supported version {supported_schema_version}"
        )));
    }
    apply_runtime_pragmas(&connection)?;
    #[cfg(test)]
    RUNTIME_CONNECTION_OPEN_COUNT.with(|count| count.set(count.get() + 1));
    Ok(runtime_connection(CachedRuntimeConnection {
        generation: CACHE_GENERATION.load(Ordering::SeqCst),
        db_path,
        supported_schema_version,
        cacheable: true,
        connection,
        _schema_guard: schema_guard,
        _maintenance_guard: guard,
    }))
}

#[cfg(test)]
thread_local! {
    static RUNTIME_CONNECTION_OPEN_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_runtime_connection_open_count() {
    RUNTIME_CONNECTION_OPEN_COUNT.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn runtime_connection_open_count() -> usize {
    RUNTIME_CONNECTION_OPEN_COUNT.with(std::cell::Cell::get)
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::{self, RecvTimeoutError};
    use std::sync::Mutex as StdMutex;
    use std::time::Duration;

    use rusqlite::Connection;
    use wc_core::config::ConfigDir;

    use super::*;

    // Process-wide runtime cache is shared across tests; serialize cache-sensitive cases.
    static CACHE_TEST_LOCK: StdMutex<()> = StdMutex::new(());

    fn config_dir() -> (tempfile::TempDir, ConfigDir) {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        (tmp, cd)
    }

    fn create_probe_db(cd: &ConfigDir, value: &str) {
        let conn = Connection::open(cd.db_path()).unwrap();
        conn.execute_batch(
            "CREATE TABLE probe (value TEXT NOT NULL);
             DELETE FROM probe;",
        )
        .unwrap();
        conn.execute("INSERT INTO probe (value) VALUES (?1)", [value])
            .unwrap();
    }

    #[test]
    fn runtime_connection_holds_shared_lock_until_connection_drop() {
        let _lock = CACHE_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        invalidate_cached_connections();
        let (_tmp, cd) = config_dir();
        create_probe_db(&cd, "old");
        // Keep the lease alive (not returned to cache) while exclusive waits.
        let runtime = open_runtime_connection(&cd, 0).unwrap();

        let competing_cd = ConfigDir {
            path: cd.path.clone(),
        };
        let (tx, rx) = mpsc::channel();
        let waiter = std::thread::spawn(move || {
            let guard = acquire_maintenance_lock(&competing_cd).unwrap();
            tx.send(guard).unwrap();
        });

        assert!(matches!(
            rx.recv_timeout(Duration::from_millis(150)),
            Err(RecvTimeoutError::Timeout)
        ));

        drop(runtime);
        let guard = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("exclusive maintenance must proceed after RuntimeConnection drops");
        drop(guard);
        waiter.join().unwrap();
    }

    #[test]
    fn runtime_connection_holds_schema_shared_lock_until_connection_drop() {
        let _lock = CACHE_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        invalidate_cached_connections();
        let (_tmp, cd) = config_dir();
        create_probe_db(&cd, "old");
        let runtime = open_runtime_connection(&cd, 0).unwrap();

        let competing_cd = ConfigDir {
            path: cd.path.clone(),
        };
        let (tx, rx) = mpsc::channel();
        let waiter = std::thread::spawn(move || {
            let guard = acquire_schema_lock(&competing_cd).unwrap();
            tx.send(guard).unwrap();
        });

        let early = rx.recv_timeout(Duration::from_millis(150));
        let was_blocked = matches!(&early, Err(RecvTimeoutError::Timeout));

        drop(runtime);
        let guard = match early {
            Ok(guard) => guard,
            Err(RecvTimeoutError::Timeout) => rx
                .recv_timeout(Duration::from_secs(2))
                .expect("exclusive schema work must proceed after RuntimeConnection drops"),
            Err(RecvTimeoutError::Disconnected) => {
                panic!("exclusive schema lock waiter disconnected")
            }
        };
        drop(guard);
        waiter.join().unwrap();

        assert!(
            was_blocked,
            "RuntimeConnection must retain a shared schema lock until its SQLite connection closes"
        );
    }

    #[test]
    fn exclusive_lock_blocks_runtime_open_until_after_database_replacement() {
        let _lock = CACHE_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        invalidate_cached_connections();
        let (_tmp, cd) = config_dir();
        create_probe_db(&cd, "old");

        let replacement = cd.path.join("replacement.db");
        {
            let conn = Connection::open(&replacement).unwrap();
            conn.execute("CREATE TABLE probe (value TEXT NOT NULL)", [])
                .unwrap();
            conn.execute("INSERT INTO probe (value) VALUES ('new')", [])
                .unwrap();
        }

        let maintenance = acquire_maintenance_lock(&cd).unwrap();
        let competing_cd = ConfigDir {
            path: cd.path.clone(),
        };
        let (tx, rx) = mpsc::channel();
        let opener = std::thread::spawn(move || {
            let conn = open_runtime_connection(&competing_cd, 0).unwrap();
            let value: String = conn
                .query_row("SELECT value FROM probe", [], |row| row.get(0))
                .unwrap();
            tx.send(value).unwrap();
        });

        assert!(matches!(
            rx.recv_timeout(Duration::from_millis(150)),
            Err(RecvTimeoutError::Timeout)
        ));
        std::fs::rename(&replacement, cd.db_path()).unwrap();
        drop(maintenance);

        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2))
                .expect("runtime open must resume after maintenance unlock"),
            "new"
        );
        opener.join().unwrap();
    }

    #[test]
    fn runtime_connection_cache_reuses_same_db_path() {
        let _lock = CACHE_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let (_tmp, cd) = config_dir();
        create_probe_db(&cd, "cached");
        invalidate_cached_connections();
        reset_runtime_connection_open_count();

        {
            let conn = open_runtime_connection(&cd, 0).unwrap();
            let value: String = conn
                .query_row("SELECT value FROM probe", [], |row| row.get(0))
                .unwrap();
            assert_eq!(value, "cached");
        }
        {
            let conn = open_runtime_connection(&cd, 0).unwrap();
            let value: String = conn
                .query_row("SELECT value FROM probe", [], |row| row.get(0))
                .unwrap();
            assert_eq!(value, "cached");
        }

        assert_eq!(
            runtime_connection_open_count(),
            1,
            "second open of the same db must reuse the cached connection"
        );
    }

    #[test]
    fn cached_connection_releases_shared_lock_when_maintenance_starts() {
        let _lock = CACHE_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let (_tmp, cd) = config_dir();
        create_probe_db(&cd, "old");
        invalidate_cached_connections();

        {
            let _conn = open_runtime_connection(&cd, 0).unwrap();
        }
        // Idle cache still holds shared locks until exclusive acquisition
        // invalidates it.
        let guard = acquire_maintenance_lock(&cd)
            .expect("maintenance must invalidate the idle cache and take exclusive");
        drop(guard);
    }

    #[test]
    fn invalidate_prevents_stale_cache_return_after_lease() {
        let _lock = CACHE_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let (_tmp, cd) = config_dir();
        create_probe_db(&cd, "old");
        invalidate_cached_connections();
        reset_runtime_connection_open_count();

        let leased = open_runtime_connection(&cd, 0).unwrap();
        invalidate_cached_connections();
        drop(leased);

        let _again = open_runtime_connection(&cd, 0).unwrap();
        assert_eq!(
            runtime_connection_open_count(),
            2,
            "invalidated lease must not return to cache"
        );
    }
}
