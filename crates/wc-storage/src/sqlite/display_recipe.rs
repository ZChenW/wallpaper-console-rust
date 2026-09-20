//! Recipe persistence and migration, alongside the legacy display-state projection.
use super::DisplayStateTarget;
use crate::sqlite_err;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;
use wc_core::{
    display_assignment::{Presentation, RenderOptions, RenderRecipe},
    error::WcError,
    types::Backend,
};

pub fn capture_render_options(
    conn: &Connection,
    backend: Backend,
) -> Result<RenderOptions, WcError> {
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='config')",
            [],
            |r| r.get(0),
        )
        .map_err(sqlite_err)?;
    let mut values = HashMap::new();
    if exists {
        let mut stmt = conn
            .prepare("SELECT key,value FROM config")
            .map_err(sqlite_err)?;
        for row in stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(sqlite_err)?
        {
            let (key, value) = row.map_err(sqlite_err)?;
            values.insert(key, value);
        }
    }
    RenderOptions::capture(backend, &values)
}
fn backend(raw: &str) -> Result<Backend, WcError> {
    match raw {
        "awww" | "swww" => Ok(Backend::Awww),
        "mpvpaper" => Ok(Backend::Mpvpaper),
        "swaybg" => Ok(Backend::Swaybg),
        "feh" => Ok(Backend::Feh),
        "linux-wallpaperengine" => Ok(Backend::LinuxWallpaperEngine),
        _ => Err(WcError::Other(format!("invalid recipe backend: {raw}"))),
    }
}

pub(crate) fn ensure_recipe_columns(conn: &Connection) -> Result<(), WcError> {
    let columns: Vec<String> = conn
        .prepare("PRAGMA table_info(display_state)")
        .map_err(sqlite_err)?
        .query_map([], |r| r.get(1))
        .map_err(sqlite_err)?
        .collect::<Result<_, _>>()
        .map_err(sqlite_err)?;
    for (name, definition) in [
        ("recipe_version", "INTEGER NOT NULL DEFAULT 1"),
        ("recipe_json", "TEXT"),
        ("assignment_revision", "INTEGER NOT NULL DEFAULT 1"),
        (
            "recipe_provenance",
            "TEXT NOT NULL DEFAULT 'migrated_defaults'",
        ),
    ] {
        if !columns.iter().any(|c| c == name) {
            conn.execute_batch(&format!(
                "ALTER TABLE display_state ADD COLUMN {name} {definition}"
            ))
            .map_err(sqlite_err)?;
        }
    }
    Ok(())
}

// Only schema migration may backfill every legacy row. Ordinary reads must
// never reinterpret a corrupt v8 assignment using today's defaults.
pub(crate) fn fill_missing_recipes(conn: &Connection) -> Result<(), WcError> {
    for (key, _, _) in missing_recipes(conn)? {
        initialize_recipe(conn, &key)?;
    }
    Ok(())
}
fn missing_recipes(conn: &Connection) -> Result<Vec<(String, String, String)>, WcError> {
    conn.prepare(
        "SELECT target_key,wallpaper_path,backend FROM display_state WHERE recipe_json IS NULL",
    )
    .map_err(sqlite_err)?
    .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
    .map_err(sqlite_err)?
    .collect::<Result<_, _>>()
    .map_err(sqlite_err)
}
pub(crate) fn initialize_recipe(conn: &Connection, key: &str) -> Result<(), WcError> {
    let (path, raw): (String, String) = conn
        .query_row(
            "SELECT wallpaper_path,backend FROM display_state WHERE target_key=?1",
            [key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(sqlite_err)?;
    let recipe = RenderRecipe {
        schema_version: 1,
        source: path.clone(),
        media_path: path,
        presentation: Presentation::Original,
        options: capture_render_options(conn, backend(&raw)?)?,
    };
    let json = serde_json::to_string(&recipe).map_err(|e| WcError::Other(e.to_string()))?;
    conn.execute(
        "UPDATE display_state SET recipe_json=?1 WHERE target_key=?2",
        params![json, key],
    )
    .map_err(sqlite_err)?;
    Ok(())
}

pub fn display_recipe(
    conn: &Connection,
    target: &DisplayStateTarget,
) -> Result<Option<RenderRecipe>, WcError> {
    let row: Option<(i64,Option<String>,String,String)> = conn.query_row(
        "SELECT recipe_version,recipe_json,wallpaper_path,backend FROM display_state WHERE target_key=?1",
        [target.storage_key()], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(sqlite_err)?;
    let Some((version, json, path, raw_backend)) = row else {
        return Ok(None);
    };
    if version != 1 {
        return Err(WcError::Other(format!(
            "unsupported render recipe version {version}"
        )));
    }
    let recipe: RenderRecipe = serde_json::from_str(
        &json.ok_or_else(|| WcError::Other("saved recipe is missing; repair assignment".into()))?,
    )
    .map_err(|e| WcError::Other(format!("corrupt render recipe: {e}")))?;
    recipe.validate()?;
    // Preview assignments project the resolved media, originals project their source.
    let projection = &recipe.media_path;
    if projection != &path || recipe.options.backend() != backend(&raw_backend)? {
        return Err(WcError::Other(
            "render recipe disagrees with display assignment".into(),
        ));
    }
    Ok(Some(recipe))
}

// Preserve opaque recipes for unchanged siblings, including future/corrupt records.
// Reading those records for execution remains fail-closed in display_recipe.
pub(crate) struct SavedRecipe {
    key: String,
    path: String,
    backend: String,
    version: i64,
    json: Option<String>,
    revision: i64,
    provenance: String,
}
pub(crate) fn snapshot(conn: &Connection) -> Result<Vec<SavedRecipe>, WcError> {
    conn.prepare("SELECT target_key,wallpaper_path,backend,recipe_version,recipe_json,assignment_revision,recipe_provenance FROM display_state")
        .map_err(sqlite_err)?.query_map([], |r| Ok(SavedRecipe {key:r.get(0)?,path:r.get(1)?,backend:r.get(2)?,version:r.get(3)?,json:r.get(4)?,revision:r.get(5)?,provenance:r.get(6)?}))
        .map_err(sqlite_err)?.collect::<Result<_,_>>().map_err(sqlite_err)
}
pub(crate) fn preserve(conn: &Connection, previous: &[SavedRecipe]) -> Result<(), WcError> {
    for row in previous {
        conn.execute("UPDATE display_state SET recipe_version=?1,recipe_json=?2,assignment_revision=?3,recipe_provenance=?4 WHERE target_key=?5 AND wallpaper_path=?6 AND backend=?7",
            params![row.version,row.json,row.revision,row.provenance,row.key,row.path,row.backend]).map_err(sqlite_err)?;
    }
    for (key, path, backend) in missing_recipes(conn)? {
        if previous
            .iter()
            .any(|row| row.key == key && row.path == path && row.backend == backend)
        {
            // Existing corrupt preferences remain corrupt; never infer their intent.
            continue;
        }
        let base = previous.iter().find(|row| {
            row.key == super::ALL_DISPLAYS_TARGET_KEY && row.path == path && row.backend == backend
        });
        if !previous.iter().any(|row| row.key == key) {
            if let Some(base) = base {
                conn.execute("UPDATE display_state SET recipe_version=?1,recipe_json=?2,assignment_revision=?3,recipe_provenance=?4 WHERE target_key=?5",
                    params![base.version,base.json,base.revision,base.provenance,key]).map_err(sqlite_err)?;
                continue;
            }
        }
        initialize_recipe(conn, &key)?;
    }
    Ok(())
}
pub(crate) fn write(
    conn: &Connection,
    target: &DisplayStateTarget,
    recipe: &RenderRecipe,
    previous: &[SavedRecipe],
) -> Result<(), WcError> {
    recipe.validate()?;
    if let Some(saved) = previous.iter().find(|r| r.key == target.storage_key()) {
        if saved.version != 1 {
            return Err(WcError::Other(format!(
                "unsupported render recipe version {}; refusing to overwrite {}",
                saved.version, saved.key
            )));
        }
    }
    let revision = previous
        .iter()
        .find(|r| r.key == target.storage_key())
        .map_or(1, |r| r.revision.saturating_add(1));
    let json = serde_json::to_string(recipe).map_err(|e| WcError::Other(e.to_string()))?;
    let n = conn.execute("UPDATE display_state SET recipe_version=1,recipe_json=?1,assignment_revision=?2,recipe_provenance='applied' WHERE target_key=?3", params![json,revision,target.storage_key()]).map_err(sqlite_err)?;
    if n != 1 {
        return Err(WcError::Other(
            "recipe target missing from assignment commit".into(),
        ));
    }
    display_recipe(conn, target)?;
    Ok(())
}

/// Stable fingerprint of saved assignment revisions for queued-op rechecks.
pub fn assignment_revision_fingerprint(
    conn: &Connection,
) -> Result<Vec<(String, i64)>, WcError> {
    conn.prepare(
        "SELECT target_key, assignment_revision FROM display_state ORDER BY target_key",
    )
    .map_err(sqlite_err)?
    .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))
    .map_err(sqlite_err)?
    .collect::<Result<Vec<_>, _>>()
    .map_err(sqlite_err)
}
