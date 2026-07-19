//! Shared conversion from `rusqlite::Error` into [`WcError`].
//!
//! Keeps SQLite extended error codes in the message for diagnostics while
//! preserving the public `WcError::Sqlite(String)` Display shape
//! (`SQLite error: ...`).

use rusqlite::Error as RusqliteError;
use wc_core::error::WcError;

/// Convert a rusqlite error into [`WcError::Sqlite`], appending the SQLite
/// error code when available (e.g. `"database is locked (code: DatabaseBusy)"`).
pub fn sqlite_err(error: RusqliteError) -> WcError {
    let message = match error.sqlite_error_code() {
        Some(code) => format!("{error} (code: {code:?})"),
        None => error.to_string(),
    };
    WcError::Sqlite(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_err_display_keeps_sqlite_error_prefix() {
        let err = sqlite_err(RusqliteError::InvalidQuery);
        let display = err.to_string();
        assert!(
            display.starts_with("SQLite error: "),
            "Display must stay compatible with existing prefixes, got: {display}"
        );
    }
}
