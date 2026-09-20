//! Bounded operation progress, separate from saved assignments and library data.
use crate::sqlite_err;
use rusqlite::{params, Connection};
use wc_core::error::WcError;

pub(crate) fn ensure_schema(conn: &Connection) -> Result<(), WcError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS display_operations (
        operation_id TEXT PRIMARY KEY NOT NULL,
        session_id TEXT NOT NULL,
        phase TEXT NOT NULL,
        payload TEXT NOT NULL,
        updated_at TEXT NOT NULL DEFAULT (datetime('now'))
    );
    CREATE TABLE IF NOT EXISTS display_stop_intents (
        session_id TEXT NOT NULL,
        output TEXT NOT NULL,
        epoch INTEGER NOT NULL DEFAULT 1,
        stopped INTEGER NOT NULL CHECK (stopped IN (0,1)),
        PRIMARY KEY (session_id, output)
    );",
    )
    .map_err(sqlite_err)
}

fn checked_payload(payload: &str) -> Result<(), WcError> {
    if payload.len() > 256 * 1024 {
        return Err(WcError::Other(
            "display operation record exceeds 256 KiB".into(),
        ));
    }
    Ok(())
}

pub fn begin(conn: &Connection, id: &str, session: &str, payload: &str) -> Result<(), WcError> {
    checked_payload(payload)?;
    let tx = conn.unchecked_transaction().map_err(sqlite_err)?;
    let count: i64 = tx
        .query_row(
            "SELECT count(*) FROM display_operations WHERE phase='executing'",
            [],
            |r| r.get(0),
        )
        .map_err(sqlite_err)?;
    if count >= 32 {
        return Err(WcError::Other(
            "too many unreconciled display operations".into(),
        ));
    }
    tx.execute("INSERT INTO display_operations(operation_id,session_id,phase,payload) VALUES (?1,?2,'executing',?3)", params![id,session,payload]).map_err(sqlite_err)?;
    tx.commit().map_err(sqlite_err)
}

pub fn finish(conn: &Connection, id: &str, payload: &str) -> Result<(), WcError> {
    checked_payload(payload)?;
    let tx = conn.unchecked_transaction().map_err(sqlite_err)?;
    tx.execute("UPDATE display_operations SET phase='finished',payload=?2,updated_at=datetime('now') WHERE operation_id=?1", params![id,payload]).map_err(sqlite_err)?;
    tx.execute("DELETE FROM display_operations WHERE phase <> 'executing' AND operation_id NOT IN
        (SELECT operation_id FROM display_operations WHERE phase <> 'executing' ORDER BY rowid DESC LIMIT 64)", []).map_err(sqlite_err)?;
    tx.commit().map_err(sqlite_err)
}

/// A fresh observation closes abandoned progress without signalling any cached PID.
pub fn reconcile_interrupted(
    conn: &Connection,
    session: &str,
    observation: &str,
) -> Result<(), WcError> {
    checked_payload(observation)?;
    conn.execute("UPDATE display_operations SET phase='interrupted_observed',payload=json_set(payload,'$.interruptedObservation',json(?2)),updated_at=datetime('now') WHERE session_id=?1 AND phase='executing'", params![session, observation]).map_err(sqlite_err)?;
    Ok(())
}

pub fn set_stopped(
    conn: &Connection,
    session: &str,
    outputs: &[String],
    stopped: bool,
) -> Result<(), WcError> {
    let tx = conn.unchecked_transaction().map_err(sqlite_err)?;
    for output in outputs {
        tx.execute(
            "INSERT INTO display_stop_intents(session_id,output,stopped) VALUES (?1,?2,?3)
            ON CONFLICT(session_id,output) DO UPDATE SET stopped=excluded.stopped,epoch=epoch+1",
            params![session, output, stopped],
        )
        .map_err(sqlite_err)?;
    }
    tx.commit().map_err(sqlite_err)
}

pub fn stop_intents(conn: &Connection, session: &str) -> Result<Vec<(String, i64, bool)>, WcError> {
    conn.prepare("SELECT output,epoch,stopped FROM display_stop_intents WHERE session_id=?1")
        .map_err(sqlite_err)?
        .query_map([session], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .map_err(sqlite_err)?
        .collect::<Result<_, _>>()
        .map_err(sqlite_err)
}
