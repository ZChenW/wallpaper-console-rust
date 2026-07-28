use std::collections::HashSet;
use std::fs;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};

#[cfg(test)]
pub(crate) const STEAM_LIBRARY_FOLDERS_SIZE_CAP: usize = 1024 * 1024;
#[cfg(not(test))]
const STEAM_LIBRARY_FOLDERS_SIZE_CAP: usize = 1024 * 1024;
const STEAM_LIBRARY_FOLDERS_TOKEN_CAP: usize = 100_000;
const STEAM_LIBRARY_FOLDERS_DEPTH_CAP: usize = 32;

fn steam_install_root_candidates_with_xdg_data_home(
    home: &Path,
    xdg_data_home: Option<&Path>,
) -> Vec<PathBuf> {
    let mut roots = [
        ".local/share/Steam",
        ".steam/steam",
        ".steam/root",
        "Steam",
        ".var/app/com.valvesoftware.Steam/.local/share/Steam",
        ".var/app/com.valvesoftware.Steam/.steam/steam",
        ".var/app/com.valvesoftware.Steam/.steam/root",
        ".var/app/com.valvesoftware.Steam/data/Steam",
    ]
    .into_iter()
    .map(|base| home.join(base))
    .collect::<Vec<_>>();
    if let Some(xdg_data_home) = xdg_data_home.filter(|path| path.is_absolute()) {
        roots.push(xdg_data_home.join("Steam"));
    }
    roots
}

/// Candidate Wallpaper Engine workshop roots for common native and Flatpak
/// Steam installs under `home`.
pub fn steam_workshop_root_candidates(home: &Path) -> Vec<PathBuf> {
    steam_install_root_candidates_with_xdg_data_home(home, None)
        .into_iter()
        .map(|base| base.join("steamapps/workshop/content/431960"))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum VdfToken {
    Text(String),
    Open,
    Close,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum VdfValue {
    Text(String),
    Object(Vec<(String, VdfValue)>),
}

fn tokenize_vdf(input: &str) -> Option<Vec<VdfToken>> {
    let mut chars = input
        .strip_prefix('\u{feff}')
        .unwrap_or(input)
        .chars()
        .peekable();
    let mut tokens = Vec::new();
    while let Some(ch) = chars.next() {
        match ch {
            ch if ch.is_whitespace() => {}
            '/' if chars.peek() == Some(&'/') => {
                chars.next();
                for comment in chars.by_ref() {
                    if comment == '\n' {
                        break;
                    }
                }
            }
            '{' => tokens.push(VdfToken::Open),
            '}' => tokens.push(VdfToken::Close),
            '"' => {
                let mut text = String::new();
                let mut closed = false;
                while let Some(value) = chars.next() {
                    match value {
                        '"' => {
                            closed = true;
                            break;
                        }
                        '\\' => match chars.next()? {
                            '"' => text.push('"'),
                            '\\' => text.push('\\'),
                            'n' => text.push('\n'),
                            'r' => text.push('\r'),
                            't' => text.push('\t'),
                            escaped => {
                                text.push('\\');
                                text.push(escaped);
                            }
                        },
                        other => text.push(other),
                    }
                }
                if !closed {
                    return None;
                }
                tokens.push(VdfToken::Text(text));
            }
            first => {
                let mut text = String::from(first);
                while let Some(&next) = chars.peek() {
                    if next.is_whitespace() || matches!(next, '{' | '}') {
                        break;
                    }
                    text.push(next);
                    chars.next();
                }
                tokens.push(VdfToken::Text(text));
            }
        }
        if tokens.len() > STEAM_LIBRARY_FOLDERS_TOKEN_CAP {
            return None;
        }
    }
    Some(tokens)
}

fn parse_vdf_value(tokens: &[VdfToken], index: &mut usize, depth: usize) -> Option<VdfValue> {
    if depth > STEAM_LIBRARY_FOLDERS_DEPTH_CAP {
        return None;
    }
    match tokens.get(*index)? {
        VdfToken::Text(text) => {
            *index += 1;
            Some(VdfValue::Text(text.clone()))
        }
        VdfToken::Open => {
            *index += 1;
            let mut entries = Vec::new();
            loop {
                match tokens.get(*index)? {
                    VdfToken::Close => {
                        *index += 1;
                        return Some(VdfValue::Object(entries));
                    }
                    VdfToken::Text(key) => {
                        let key = key.clone();
                        *index += 1;
                        entries.push((key, parse_vdf_value(tokens, index, depth + 1)?));
                    }
                    VdfToken::Open => return None,
                }
            }
        }
        VdfToken::Close => None,
    }
}

fn steam_library_paths_from_vdf(input: &str) -> Option<Vec<PathBuf>> {
    let tokens = tokenize_vdf(input)?;
    let mut index = 0;
    let VdfToken::Text(root_name) = tokens.get(index)? else {
        return None;
    };
    if !root_name.eq_ignore_ascii_case("libraryfolders") {
        return None;
    }
    index += 1;
    let VdfValue::Object(entries) = parse_vdf_value(&tokens, &mut index, 0)? else {
        return None;
    };
    if index != tokens.len() {
        return None;
    }

    Some(
        entries
            .into_iter()
            .filter(|(key, _)| !key.is_empty() && key.chars().all(|ch| ch.is_ascii_digit()))
            .filter_map(|(_, value)| match value {
                VdfValue::Text(path) => Some(path),
                VdfValue::Object(fields) => fields.into_iter().find_map(|(key, value)| {
                    if key.eq_ignore_ascii_case("path") {
                        if let VdfValue::Text(path) = value {
                            return Some(path);
                        }
                    }
                    None
                }),
            })
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .collect(),
    )
}

fn read_steam_library_folders(path: &Path) -> Result<Option<String>, String> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot open: {error}")),
    };
    let size = file
        .metadata()
        .map_err(|error| format!("cannot inspect: {error}"))?
        .len();
    if size > STEAM_LIBRARY_FOLDERS_SIZE_CAP as u64 {
        return Err(format!(
            "file is larger than the {} byte safety limit",
            STEAM_LIBRARY_FOLDERS_SIZE_CAP
        ));
    }
    let mut contents = Vec::new();
    file.take((STEAM_LIBRARY_FOLDERS_SIZE_CAP + 1) as u64)
        .read_to_end(&mut contents)
        .map_err(|error| format!("cannot read: {error}"))?;
    if contents.len() > STEAM_LIBRARY_FOLDERS_SIZE_CAP {
        return Err(format!(
            "file grew beyond the {} byte safety limit while reading",
            STEAM_LIBRARY_FOLDERS_SIZE_CAP
        ));
    }
    String::from_utf8(contents)
        .map(Some)
        .map_err(|error| format!("is not valid UTF-8: {error}"))
}

fn configured_steam_library_roots(home: &Path, xdg_data_home: Option<&Path>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for steam_root in steam_install_root_candidates_with_xdg_data_home(home, xdg_data_home) {
        for relative in ["config/libraryfolders.vdf", "steamapps/libraryfolders.vdf"] {
            let library_folders = steam_root.join(relative);
            let contents = match read_steam_library_folders(&library_folders) {
                Ok(Some(contents)) => contents,
                Ok(None) => continue,
                Err(error) => {
                    log::warn!(
                        "Ignoring Steam library configuration {}: {error}",
                        library_folders.display()
                    );
                    continue;
                }
            };
            let Some(paths) = steam_library_paths_from_vdf(&contents) else {
                log::warn!(
                    "Ignoring malformed Steam library configuration {}",
                    library_folders.display()
                );
                continue;
            };
            roots.extend(paths);
        }
    }
    roots
}

/// Discover existing Wallpaper Engine workshop roots, canonicalized and
/// deduplicated. This is shared by CLI and GUI so both ingest paths behave the
/// same way.
pub fn discover_steam_workshop_roots(home: &Path) -> Vec<PathBuf> {
    let xdg_data_home = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from);
    discover_steam_workshop_roots_with_xdg_data_home(home, xdg_data_home.as_deref())
}

pub(crate) fn discover_steam_workshop_roots_with_xdg_data_home(
    home: &Path,
    xdg_data_home: Option<&Path>,
) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut roots = Vec::new();
    let candidates = steam_install_root_candidates_with_xdg_data_home(home, xdg_data_home)
        .into_iter()
        .map(|base| base.join("steamapps/workshop/content/431960"))
        .chain(
            configured_steam_library_roots(home, xdg_data_home)
                .into_iter()
                .map(|library| library.join("steamapps/workshop/content/431960")),
        );
    for candidate in candidates {
        let canonical = fs::canonicalize(&candidate).unwrap_or(candidate);
        if canonical.is_dir() && seen.insert(canonical.clone()) {
            roots.push(canonical);
        }
    }
    roots
}
