use crate::sqlite_err;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use wc_core::config::ConfigDir;
use wc_core::error::WcError;

use super::schema::open_runtime_connection;

pub const SOURCE_FRESH_TTL_SECONDS: i64 = 30 * 60;
const RETRY_DELAYS_SECONDS: [i64; 5] = [60, 120, 300, 600, 1_800];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshIntent {
    Background,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceRefreshEligibility {
    Due,
    SkipFresh,
    SkipBackoff { retry_at: i64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRefreshState {
    pub source_id: i64,
    pub last_success_at: Option<i64>,
    pub dirty: bool,
    pub failure_category: Option<String>,
    pub consecutive_failures: u32,
    pub next_retry_at: Option<i64>,
}

impl SourceRefreshState {
    fn new_dirty(source_id: i64) -> Self {
        Self {
            source_id,
            last_success_at: None,
            dirty: true,
            failure_category: None,
            consecutive_failures: 0,
            next_retry_at: None,
        }
    }
}

fn read_state(connection: &Connection, source_id: i64) -> Result<SourceRefreshState, WcError> {
    let state = connection
        .query_row(
            "SELECT source_id, last_success_at, dirty, failure_category,
                    consecutive_failures, next_retry_at
             FROM source_refresh_state WHERE source_id = ?1",
            [source_id],
            |row| {
                let dirty = row.get::<_, i64>(2)?;
                let failures = row.get::<_, i64>(4)?;
                Ok(SourceRefreshState {
                    source_id: row.get(0)?,
                    last_success_at: row.get(1)?,
                    dirty: dirty != 0,
                    failure_category: row.get(3)?,
                    consecutive_failures: u32::try_from(failures.max(0)).unwrap_or(u32::MAX),
                    next_retry_at: row.get(5)?,
                })
            },
        )
        .optional()
        .map_err(sqlite_err)?;
    if let Some(state) = state {
        return Ok(state);
    }
    let exists = connection
        .query_row("SELECT 1 FROM sources WHERE id = ?1", [source_id], |_| {
            Ok(())
        })
        .optional()
        .map_err(sqlite_err)?
        .is_some();
    if !exists {
        return Err(WcError::Other(format!("unknown source id: {source_id}")));
    }
    Ok(SourceRefreshState::new_dirty(source_id))
}

pub fn read_source_refresh_state(
    cd: &ConfigDir,
    source_id: i64,
) -> Result<SourceRefreshState, WcError> {
    let connection = open_runtime_connection(cd)?;
    read_state(&connection, source_id)
}

pub fn mark_source_refresh_dirty(cd: &ConfigDir, source_id: i64) -> Result<(), WcError> {
    let connection = open_runtime_connection(cd)?;
    read_state(&connection, source_id)?;
    connection
        .execute(
            "INSERT INTO source_refresh_state (source_id, dirty)
             VALUES (?1, 1)
             ON CONFLICT(source_id) DO UPDATE SET dirty = 1",
            [source_id],
        )
        .map_err(sqlite_err)?;
    Ok(())
}

/// Make a recovered source immediately eligible for a background refresh
/// without erasing its failure history. A real refresh success remains the
/// only transition that clears the failure category and count.
pub fn mark_source_refresh_recovery_due(cd: &ConfigDir, source_id: i64) -> Result<(), WcError> {
    let mut connection = open_runtime_connection(cd)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_err)?;
    read_state(&transaction, source_id)?;
    transaction
        .execute(
            "INSERT INTO source_refresh_state (source_id, dirty, next_retry_at)
             VALUES (?1, 1, NULL)
             ON CONFLICT(source_id) DO UPDATE SET
                 dirty = 1,
                 next_retry_at = NULL",
            [source_id],
        )
        .map_err(sqlite_err)?;
    transaction.commit().map_err(sqlite_err)?;
    Ok(())
}

/// Clear the dirty bit immediately before enumeration begins. Any watcher
/// event arriving during the scan sets it again, and success deliberately
/// preserves that newer bit so a follow-up cannot be lost.
pub fn begin_source_refresh_attempt(cd: &ConfigDir, source_id: i64) -> Result<(), WcError> {
    let connection = open_runtime_connection(cd)?;
    read_state(&connection, source_id)?;
    connection
        .execute(
            "INSERT INTO source_refresh_state (source_id, dirty)
             VALUES (?1, 0)
             ON CONFLICT(source_id) DO UPDATE SET dirty = 0",
            [source_id],
        )
        .map_err(sqlite_err)?;
    Ok(())
}

pub fn record_source_refresh_success(
    cd: &ConfigDir,
    source_id: i64,
    now: i64,
) -> Result<SourceRefreshState, WcError> {
    let connection = open_runtime_connection(cd)?;
    read_state(&connection, source_id)?;
    connection
        .execute(
            "INSERT INTO source_refresh_state
                 (source_id, last_success_at, dirty, failure_category,
                  consecutive_failures, next_retry_at)
             VALUES (?1, ?2, 0, NULL, 0, NULL)
             ON CONFLICT(source_id) DO UPDATE SET
                 last_success_at = excluded.last_success_at,
                 failure_category = NULL,
                 consecutive_failures = 0,
                 next_retry_at = NULL",
            params![source_id, now],
        )
        .map_err(sqlite_err)?;
    read_state(&connection, source_id)
}

pub fn record_source_refresh_failure(
    cd: &ConfigDir,
    source_id: i64,
    category: &str,
    now: i64,
) -> Result<SourceRefreshState, WcError> {
    let category = category.trim();
    if category.is_empty() {
        return Err(WcError::Other(
            "source refresh failure category must not be blank".into(),
        ));
    }
    let mut connection = open_runtime_connection(cd)?;
    let current = read_state(&connection, source_id)?;
    let failures = current.consecutive_failures.saturating_add(1);
    let delay_index = usize::try_from(failures.saturating_sub(1))
        .unwrap_or(usize::MAX)
        .min(RETRY_DELAYS_SECONDS.len() - 1);
    let retry_at = now.saturating_add(RETRY_DELAYS_SECONDS[delay_index]);
    let transaction = connection.transaction().map_err(sqlite_err)?;
    transaction
        .execute(
            "INSERT INTO source_refresh_state
                 (source_id, last_success_at, dirty, failure_category,
                  consecutive_failures, next_retry_at)
             VALUES (?1, ?2, 1, ?3, ?4, ?5)
             ON CONFLICT(source_id) DO UPDATE SET
                 dirty = 1,
                 failure_category = excluded.failure_category,
                 consecutive_failures = excluded.consecutive_failures,
                 next_retry_at = excluded.next_retry_at",
            params![
                source_id,
                current.last_success_at,
                category,
                i64::from(failures),
                retry_at
            ],
        )
        .map_err(sqlite_err)?;
    transaction.commit().map_err(sqlite_err)?;
    read_source_refresh_state(cd, source_id)
}

pub fn source_refresh_eligibility(
    cd: &ConfigDir,
    source_id: i64,
    now: i64,
    intent: RefreshIntent,
) -> Result<SourceRefreshEligibility, WcError> {
    if intent == RefreshIntent::Manual {
        return Ok(SourceRefreshEligibility::Due);
    }
    let state = read_source_refresh_state(cd, source_id)?;
    if let Some(retry_at) = state.next_retry_at {
        if now < retry_at {
            return Ok(SourceRefreshEligibility::SkipBackoff { retry_at });
        }
    }
    if state.dirty {
        return Ok(SourceRefreshEligibility::Due);
    }
    if state
        .last_success_at
        .is_some_and(|success| now < success.saturating_add(SOURCE_FRESH_TTL_SECONDS))
    {
        return Ok(SourceRefreshEligibility::SkipFresh);
    }
    Ok(SourceRefreshEligibility::Due)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wc_config::ConfigDirExt;
    use wc_core::config::ConfigDir;

    fn source() -> (tempfile::TempDir, ConfigDir, i64) {
        let tmp = tempfile::tempdir().unwrap();
        let cd = ConfigDir {
            path: tmp.path().join("wallpaper-console"),
        };
        cd.init().unwrap();
        crate::sqlite::try_ensure_sqlite_db(&cd).unwrap();
        let (source, _) = crate::sqlite::source_create(&cd, "/walls").unwrap();
        (tmp, cd, source.id)
    }

    #[test]
    fn success_is_fresh_for_thirty_minutes_but_manual_bypasses_ttl() {
        let (_tmp, cd, source_id) = source();
        record_source_refresh_success(&cd, source_id, 1_000).unwrap();

        assert_eq!(
            source_refresh_eligibility(&cd, source_id, 2_799, RefreshIntent::Background).unwrap(),
            SourceRefreshEligibility::SkipFresh
        );
        assert_eq!(
            source_refresh_eligibility(&cd, source_id, 2_800, RefreshIntent::Background).unwrap(),
            SourceRefreshEligibility::Due
        );
        assert_eq!(
            source_refresh_eligibility(&cd, source_id, 1_001, RefreshIntent::Manual).unwrap(),
            SourceRefreshEligibility::Due
        );
    }

    #[test]
    fn failures_follow_bounded_backoff_and_success_resets_the_cycle() {
        let (_tmp, cd, source_id) = source();
        let delays = [60, 120, 300, 600, 1_800, 1_800];
        let mut now = 10_000;
        for (index, delay) in delays.into_iter().enumerate() {
            let state = record_source_refresh_failure(&cd, source_id, "offline", now).unwrap();
            assert_eq!(state.consecutive_failures, (index + 1) as u32);
            assert_eq!(state.next_retry_at, Some(now + delay));
            assert_eq!(
                source_refresh_eligibility(
                    &cd,
                    source_id,
                    now + delay - 1,
                    RefreshIntent::Background,
                )
                .unwrap(),
                SourceRefreshEligibility::SkipBackoff {
                    retry_at: now + delay
                }
            );
            now += delay;
        }

        begin_source_refresh_attempt(&cd, source_id).unwrap();
        record_source_refresh_success(&cd, source_id, now).unwrap();
        let state = read_source_refresh_state(&cd, source_id).unwrap();
        assert_eq!(state.consecutive_failures, 0);
        assert_eq!(state.failure_category, None);
        assert_eq!(state.next_retry_at, None);
        assert!(!state.dirty);
    }

    #[test]
    fn dirty_source_is_due_when_not_held_by_failure_backoff() {
        let (_tmp, cd, source_id) = source();
        record_source_refresh_success(&cd, source_id, 1_000).unwrap();
        mark_source_refresh_dirty(&cd, source_id).unwrap();
        assert_eq!(
            source_refresh_eligibility(&cd, source_id, 1_001, RefreshIntent::Background).unwrap(),
            SourceRefreshEligibility::Due
        );
    }

    #[test]
    fn recovery_due_bypasses_backoff_without_clearing_failure_history() {
        let (_tmp, cd, source_id) = source();
        record_source_refresh_success(&cd, source_id, 9_000).unwrap();
        let failed = record_source_refresh_failure(&cd, source_id, "offline", 10_000).unwrap();
        assert_eq!(failed.next_retry_at, Some(10_060));
        assert_eq!(
            source_refresh_eligibility(&cd, source_id, 10_001, RefreshIntent::Background).unwrap(),
            SourceRefreshEligibility::SkipBackoff { retry_at: 10_060 }
        );

        mark_source_refresh_recovery_due(&cd, source_id).unwrap();

        let recovered = read_source_refresh_state(&cd, source_id).unwrap();
        assert!(recovered.dirty);
        assert_eq!(recovered.next_retry_at, None);
        assert_eq!(recovered.failure_category.as_deref(), Some("offline"));
        assert_eq!(recovered.consecutive_failures, 1);
        assert_eq!(
            source_refresh_eligibility(&cd, source_id, 10_001, RefreshIntent::Background).unwrap(),
            SourceRefreshEligibility::Due
        );
    }

    #[test]
    fn recovery_due_rejects_unknown_source() {
        let (_tmp, cd, source_id) = source();

        assert!(mark_source_refresh_recovery_due(&cd, source_id + 1).is_err());
    }

    #[test]
    fn recovery_due_propagates_write_errors_without_mutating_state() {
        let (_tmp, cd, source_id) = source();
        record_source_refresh_success(&cd, source_id, 9_000).unwrap();
        let failed = record_source_refresh_failure(&cd, source_id, "offline", 10_000).unwrap();
        assert_eq!(failed.next_retry_at, Some(10_060));
        assert_eq!(
            source_refresh_eligibility(&cd, source_id, 10_001, RefreshIntent::Background).unwrap(),
            SourceRefreshEligibility::SkipBackoff { retry_at: 10_060 }
        );

        let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
        conn.execute(
            "ALTER TABLE source_refresh_state RENAME TO source_refresh_state_backup",
            [],
        )
        .unwrap();
        drop(conn);

        assert!(mark_source_refresh_recovery_due(&cd, source_id).is_err());

        let conn = rusqlite::Connection::open(cd.db_path()).unwrap();
        conn.execute(
            "ALTER TABLE source_refresh_state_backup RENAME TO source_refresh_state",
            [],
        )
        .unwrap();
        drop(conn);

        mark_source_refresh_recovery_due(&cd, source_id).unwrap();
        assert_eq!(
            source_refresh_eligibility(&cd, source_id, 10_001, RefreshIntent::Background).unwrap(),
            SourceRefreshEligibility::Due
        );
        let recovered = read_source_refresh_state(&cd, source_id).unwrap();
        assert_eq!(recovered.failure_category.as_deref(), Some("offline"));
        assert_eq!(recovered.consecutive_failures, 1);
    }

    #[test]
    fn watcher_dirty_event_during_scan_survives_success() {
        let (_tmp, cd, source_id) = source();
        begin_source_refresh_attempt(&cd, source_id).unwrap();
        mark_source_refresh_dirty(&cd, source_id).unwrap();
        record_source_refresh_success(&cd, source_id, 1_000).unwrap();

        let state = read_source_refresh_state(&cd, source_id).unwrap();
        assert!(state.dirty);
        assert_eq!(
            source_refresh_eligibility(&cd, source_id, 1_001, RefreshIntent::Background).unwrap(),
            SourceRefreshEligibility::Due
        );
    }
}
