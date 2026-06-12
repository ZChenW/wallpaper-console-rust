//! Flat-file storage helpers — read and write runtime files.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use wc_core::config::ConfigDir;
use wc_core::error::WcError;

/// Read a flat file as a list of lines (skipping empty lines).
pub fn read_lines(path: &Path) -> Result<Vec<String>, WcError> {
    if !path.exists() || fs::metadata(path).map(|m| m.len() == 0).unwrap_or(true) {
        return Ok(Vec::new());
    }
    let f = fs::File::open(path).map_err(WcError::Io)?;
    let reader = BufReader::new(f);
    let mut lines = Vec::new();
    for line in reader.lines() {
        let line = line.map_err(WcError::Io)?;
        let trimmed = line.trim().to_string();
        if !trimmed.is_empty() {
            lines.push(trimmed);
        }
    }
    Ok(lines)
}

/// Write lines to a flat file atomically.
pub fn write_lines(path: &Path, lines: &[String]) -> Result<(), WcError> {
    let tmp = path.with_extension("tmp");
    let mut f = fs::File::create(&tmp).map_err(WcError::Io)?;
    for line in lines {
        writeln!(f, "{}", line).map_err(WcError::Io)?;
    }
    fs::rename(&tmp, path).map_err(WcError::Io)?;
    Ok(())
}

/// Append a unique line if not already present. Returns true if added.
pub fn append_unique_line(path: &Path, line: &str) -> Result<bool, WcError> {
    let mut lines = read_lines(path)?;
    if lines.iter().any(|l| l == line) {
        return Ok(false);
    }
    lines.push(line.to_string());
    write_lines(path, &lines)?;
    Ok(true)
}

/// Remove all occurrences of a line.
pub fn remove_line(path: &Path, target: &str) -> Result<(), WcError> {
    let lines: Vec<String> = read_lines(path)?
        .into_iter()
        .filter(|l| l != target)
        .collect();
    write_lines(path, &lines)
}

/// Deduplicate a flat file (keeps first occurrence order).
pub fn dedupe_file(path: &Path) -> Result<(), WcError> {
    let lines = read_lines(path)?;
    let mut seen = std::collections::HashSet::new();
    let unique: Vec<String> = lines
        .into_iter()
        .filter(|l| seen.insert(l.clone()))
        .collect();
    write_lines(path, &unique)
}

/// Read a single-line file.
pub fn read_single_line(path: &Path) -> Result<Option<String>, WcError> {
    let lines = read_lines(path)?;
    Ok(lines.into_iter().next())
}

/// Write a single-line file.
pub fn write_single_line(path: &Path, content: &str) -> Result<(), WcError> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, format!("{}\n", content)).map_err(WcError::Io)?;
    fs::rename(&tmp, path).map_err(WcError::Io)?;
    Ok(())
}

/// Clear a file (truncate to empty).
pub fn clear_file(path: &Path) -> Result<(), WcError> {
    fs::write(path, "").map_err(WcError::Io)?;
    Ok(())
}

/// Resolve a path to its canonical form. Falls back to the original path on
/// any filesystem error (e.g. broken symlink or missing file).
pub fn try_canonicalize(path: &str) -> String {
    std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string())
}

// ── High-level storage operations through ConfigDir ──────────────────────

pub fn sources_list(cd: &ConfigDir) -> Result<Vec<String>, WcError> {
    let lines = read_lines(&cd.sources_path())?;
    let mut seen = std::collections::HashSet::new();
    let deduped: Vec<String> = lines
        .into_iter()
        .filter(|l| seen.insert(try_canonicalize(l)))
        .collect();
    Ok(deduped)
}

pub fn sources_add(cd: &ConfigDir, path: &str) -> Result<bool, WcError> {
    append_unique_line(&cd.sources_path(), path)
}

pub fn sources_remove(cd: &ConfigDir, path: &str) -> Result<bool, WcError> {
    let lines = read_lines(&cd.sources_path())?;
    if !lines.iter().any(|l| l == path) {
        return Ok(false);
    }
    remove_line(&cd.sources_path(), path)?;
    Ok(true)
}

pub fn favorites_list(cd: &ConfigDir) -> Result<Vec<String>, WcError> {
    read_lines(&cd.favorites_path())
}

pub fn favorites_add(cd: &ConfigDir, path: &str) -> Result<bool, WcError> {
    append_unique_line(&cd.favorites_path(), path)
}

pub fn favorites_remove(cd: &ConfigDir, path: &str) -> Result<(), WcError> {
    remove_line(&cd.favorites_path(), path)
}

pub fn favorites_has(cd: &ConfigDir, path: &str) -> Result<bool, WcError> {
    let lines = read_lines(&cd.favorites_path())?;
    Ok(lines.iter().any(|l| l == path))
}

pub fn history_list(cd: &ConfigDir) -> Result<Vec<String>, WcError> {
    let lines = read_lines(&cd.history_path())?;
    let mut seen = std::collections::HashSet::new();
    let deduped: Vec<String> = lines
        .into_iter()
        .filter(|l| seen.insert(try_canonicalize(l)))
        .collect();
    Ok(deduped)
}

pub fn history_add(cd: &ConfigDir, path: &str, max_entries: usize) -> Result<(), WcError> {
    let mut lines = read_lines(&cd.history_path())?;
    let canon = try_canonicalize(path);
    lines.retain(|l| try_canonicalize(l) != canon);
    lines.insert(0, path.to_string());
    // Trim to max
    if lines.len() > max_entries {
        lines.truncate(max_entries);
    }
    write_lines(&cd.history_path(), &lines)
}

pub fn history_clear(cd: &ConfigDir) -> Result<(), WcError> {
    clear_file(&cd.history_path())
}

pub fn history_count(cd: &ConfigDir) -> Result<usize, WcError> {
    Ok(read_lines(&cd.history_path())?.len())
}

pub fn current_read(cd: &ConfigDir) -> Result<Option<String>, WcError> {
    read_single_line(&cd.current_path())
}

pub fn current_write(cd: &ConfigDir, path: &str) -> Result<(), WcError> {
    write_single_line(&cd.current_path(), path)
}

pub fn last_backend_read(cd: &ConfigDir) -> Result<Option<String>, WcError> {
    read_single_line(&cd.last_backend_path())
}

pub fn last_backend_write(cd: &ConfigDir, backend: &str) -> Result<(), WcError> {
    write_single_line(&cd.last_backend_path(), backend)
}
