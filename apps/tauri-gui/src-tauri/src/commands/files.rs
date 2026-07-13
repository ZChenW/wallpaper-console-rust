use super::common::{fail, ok, storage, CommandResult};

/// Resolve a path for opening: directories are opened directly, files reveal their parent,
/// except WE project directories (containing project.json) which open directly.
pub(crate) fn open_location_target(path: &str) -> Result<std::path::PathBuf, String> {
    let p = std::path::Path::new(path);
    if !p.exists() {
        return Err(format!("Path does not exist: {}", path));
    }
    if p.is_dir() {
        return Ok(p.to_path_buf());
    }
    if p.is_file() {
        return Ok(p.parent().unwrap_or(p).to_path_buf());
    }
    Err(format!("Not a regular file or directory: {}", path))
}

fn terminal_spawn_command(name: &str, inner: &[String]) -> Option<(String, Vec<String>)> {
    let args: Vec<String> = match name {
        "kitty" => {
            let mut a = vec!["--".to_string()];
            a.extend_from_slice(inner);
            a
        }
        "alacritty" => {
            let mut a = vec!["-e".to_string()];
            a.extend_from_slice(inner);
            a
        }
        "foot" => inner.to_vec(),
        "wezterm" | "wezterm-204" => {
            let mut a = vec!["start".to_string(), "--".to_string()];
            a.extend_from_slice(inner);
            a
        }
        n if n.starts_with("wezterm") => {
            let mut a = vec!["start".to_string(), "--".to_string()];
            a.extend_from_slice(inner);
            a
        }
        "gnome-terminal" => {
            let mut a = vec!["--".to_string()];
            a.extend_from_slice(inner);
            a
        }
        "konsole" => {
            let mut a = vec!["-e".to_string()];
            a.extend_from_slice(inner);
            a
        }
        _ => return None,
    };
    Some((name.to_string(), args))
}

/// Return candidate terminal executable names for the current environment.
/// $TERMINAL is tried first, then well-known terminals, deduplicated.
fn terminal_candidates(term_var: Option<&str>) -> Vec<&str> {
    let mut seen = std::collections::HashSet::new();
    let mut list = Vec::new();
    let fallback = [
        "kitty",
        "alacritty",
        "foot",
        "wezterm",
        "gnome-terminal",
        "konsole",
    ];
    if let Some(term) = term_var {
        let exe = term.split_whitespace().next().unwrap_or("");
        if !exe.is_empty() && seen.insert(exe) {
            list.push(exe);
        }
    }
    for &f in &fallback {
        if seen.insert(f) {
            list.push(f);
        }
    }
    list
}

/// Split a custom command string, replacing `{path}` with target or appending it.
fn custom_command_parts(custom_cmd: &str, target: &str) -> Result<Vec<String>, String> {
    let trimmed = custom_cmd.trim();
    if trimmed.is_empty() {
        return Err("Custom command is empty.".into());
    }
    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    let mut has_placeholder = false;
    let mut result = Vec::new();
    for p in &parts {
        if *p == "{path}" {
            has_placeholder = true;
            result.push(target.to_string());
        } else {
            result.push(p.to_string());
        }
    }
    if !has_placeholder {
        result.push(target.to_string());
    }
    Ok(result)
}

/// Return candidate executable names for a given file manager config value.
fn file_manager_candidates(file_mgr: &str) -> Vec<&str> {
    match file_mgr {
        "auto" => vec!["nautilus", "dolphin", "thunar", "nemo", "pcmanfm"],
        "custom" | "" => vec![],
        other => vec![other],
    }
}

fn open_with_file_manager(
    target: &std::path::Path,
    file_mgr: &str,
    custom_cmd: &str,
) -> Result<String, String> {
    let target_str = target.to_string_lossy().to_string();

    if file_mgr == "custom" {
        let parts = custom_command_parts(custom_cmd, &target_str)?;
        let prog = parts[0].clone();
        let _status = std::process::Command::new(&prog)
            .args(&parts[1..])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to launch {}: {}", prog, e))?;
        return Ok(format!("Opened with custom: {}", target_str));
    }

    let candidates = file_manager_candidates(file_mgr);
    if candidates.is_empty() {
        return Err(
            "No file manager configured. Choose one in Settings or set a custom command.".into(),
        );
    }
    let candidate_names = candidates.join(", ");
    for c in &candidates {
        let status = std::process::Command::new(c)
            .arg(&target_str)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        match status {
            Ok(_) => return Ok(format!("Opened with {}: {}", c, target_str)),
            Err(_) => continue,
        }
    }
    Err(format!(
        "No file manager found. Tried: {}. Install one or set a custom command in Settings.",
        candidate_names
    ))
}

fn terminal_file_manager_command(
    tui_mgr: &str,
    custom_cmd: &str,
    target: &str,
) -> Result<Vec<String>, String> {
    match tui_mgr {
        "yazi" => Ok(vec!["yazi".to_string(), target.to_string()]),
        "custom" => custom_command_parts(custom_cmd, target),
        _ => Ok(vec!["yazi".to_string(), target.to_string()]),
    }
}

fn try_terminal_spawn_candidates(
    candidates: &[&str],
    tui_cmd: &[String],
    tui_label: &str,
    target_str: &str,
    spawn_fn: &mut dyn FnMut(&str, &[String]) -> bool,
) -> Result<String, String> {
    let mut attempted = Vec::new();

    for c in candidates {
        let Some((prog, args)) = terminal_spawn_command(c, tui_cmd) else {
            continue;
        };
        attempted.push(format!("{} {:?}", prog, args));
        if spawn_fn(&prog, &args) {
            return Ok(format!(
                "Opened with {} in {}: {}",
                tui_label, c, target_str
            ));
        }
    }
    Err(format!(
        "No terminal emulator could be launched. Tried: {}. Install a terminal emulator or set $TERMINAL.",
        attempted.join(", ")
    ))
}

fn open_terminal_file_manager(
    target: &std::path::Path,
    tui_mgr: &str,
    custom_cmd: &str,
) -> Result<String, String> {
    let target_str = target.to_string_lossy().to_string();
    let term = std::env::var("TERMINAL").ok();
    let terminal_candidates = terminal_candidates(term.as_deref());
    let tui_cmd = terminal_file_manager_command(tui_mgr, custom_cmd, &target_str)?;

    try_terminal_spawn_candidates(
        &terminal_candidates,
        &tui_cmd,
        tui_mgr,
        &target_str,
        &mut |prog: &str, args: &[String]| {
            std::process::Command::new(prog)
                .args(args)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .is_ok()
        },
    )
}

#[tauri::command]
pub async fn open_project_location(path: String, mode: Option<String>) -> CommandResult {
    tauri::async_runtime::spawn_blocking(move || {
        let target = match open_location_target(&path) {
            Ok(t) => t,
            Err(e) => return fail(e),
        };
        let s = match storage() {
            Ok(s) => s,
            Err(e) => return fail(e),
        };
        let mode = mode.unwrap_or_else(|| s.config_get("open_project_location_mode", "ask"));
        if mode == "ask" {
            return fail(
                "Open location mode is ask-on-first-use. \
                 Choose File Manager or Terminal File Manager in Settings first.",
            );
        }
        if mode == "terminal" {
            let tui_mgr = s.config_get("gui_terminal_file_manager", "yazi");
            let custom = s.config_get("gui_terminal_file_manager_custom", "");
            match open_terminal_file_manager(&target, &tui_mgr, &custom) {
                Ok(msg) => ok(msg),
                Err(e) => fail(e),
            }
        } else {
            let file_mgr = s.config_get("gui_file_manager", "auto");
            let custom = s.config_get("gui_file_manager_custom", "");
            match open_with_file_manager(&target, &file_mgr, &custom) {
                Ok(msg) => ok(msg),
                Err(e) => fail(e),
            }
        }
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn open_path(path: String) -> CommandResult {
    tauri::async_runtime::spawn_blocking(move || {
        let p = std::path::Path::new(&path);
        // If it's a directory, open it directly; otherwise reveal parent.
        let target = if p.is_dir() {
            p.to_path_buf()
        } else {
            p.parent().unwrap_or(p).to_path_buf()
        };
        match std::process::Command::new("xdg-open")
            .arg(target.to_string_lossy().as_ref())
            .spawn()
        {
            Ok(_) => ok("Opened path."),
            Err(e) => fail(e.to_string()),
        }
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[tauri::command]
pub async fn reveal_in_file_manager(path: String) -> CommandResult {
    tauri::async_runtime::spawn_blocking(move || {
        let p = std::path::Path::new(&path);
        let target = if p.is_dir() {
            p.to_path_buf()
        } else {
            p.parent().unwrap_or(p).to_path_buf()
        };
        let s = match storage() {
            Ok(s) => s,
            Err(e) => return fail(e),
        };
        let file_mgr = s.config_get("gui_file_manager", "auto");
        let custom = s.config_get("gui_file_manager_custom", "");
        match open_with_file_manager(&target, &file_mgr, &custom) {
            Ok(msg) => ok(msg),
            Err(e) => fail(e),
        }
    })
    .await
    .unwrap_or_else(|e| fail(e.to_string()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DirectoryPickerOutput {
    success: bool,
    stdout: String,
    stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DirectoryPickerRunError {
    NotFound,
    Failed(String),
}

fn directory_picker_candidates() -> Vec<(&'static str, Vec<String>)> {
    let title = "Choose wallpaper folder";
    let start_dir = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    vec![
        (
            "zenity",
            vec![
                "--file-selection".into(),
                "--directory".into(),
                format!("--title={title}"),
            ],
        ),
        (
            "kdialog",
            vec![
                "--title".into(),
                title.into(),
                "--getexistingdirectory".into(),
                start_dir,
            ],
        ),
        (
            "yad",
            vec![
                "--file-selection".into(),
                "--directory".into(),
                format!("--title={title}"),
            ],
        ),
    ]
}

fn pick_directory_with<R, D>(mut run: R, is_directory: D) -> Result<String, String>
where
    R: FnMut(&str, &[String]) -> Result<DirectoryPickerOutput, DirectoryPickerRunError>,
    D: Fn(&std::path::Path) -> bool,
{
    for (program, args) in directory_picker_candidates() {
        let output = match run(program, &args) {
            Ok(output) => output,
            Err(DirectoryPickerRunError::NotFound) => continue,
            Err(DirectoryPickerRunError::Failed(error)) => {
                return Err(format!(
                    "Could not start {program} directory picker: {error}"
                ));
            }
        };

        if !output.success {
            let detail = output.stderr.trim();
            return if detail.is_empty() {
                Ok(String::new())
            } else {
                Err(format!("{program} directory picker failed: {detail}"))
            };
        }

        let selected = output.stdout.trim();
        if selected.is_empty() {
            return Ok(String::new());
        }
        let selected_path = std::path::Path::new(selected);
        if !is_directory(selected_path) {
            return Err(format!(
                "Directory picker returned a path that is not an existing directory: {selected}"
            ));
        }
        return Ok(selected.to_string());
    }

    Err(
        "No supported directory picker is installed. Install zenity, kdialog, or yad and try again."
            .into(),
    )
}

fn pick_directory() -> Result<String, String> {
    pick_directory_with(
        |program, args| {
            std::process::Command::new(program)
                .args(args)
                .output()
                .map(|output| DirectoryPickerOutput {
                    success: output.status.success(),
                    stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                })
                .map_err(|error| {
                    if error.kind() == std::io::ErrorKind::NotFound {
                        DirectoryPickerRunError::NotFound
                    } else {
                        DirectoryPickerRunError::Failed(error.to_string())
                    }
                })
        },
        std::path::Path::is_dir,
    )
}

#[tauri::command]
pub async fn browse_directory(app: tauri::AppHandle) -> Result<String, String> {
    let _ = app;
    tauri::async_runtime::spawn_blocking(pick_directory)
        .await
        .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picker_output(success: bool, stdout: &str, stderr: &str) -> DirectoryPickerOutput {
        DirectoryPickerOutput {
            success,
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }

    #[test]
    fn directory_picker_returns_the_first_valid_selection() {
        let tmp = tempfile::tempdir().unwrap();
        let selected = tmp.path().to_string_lossy().into_owned();
        let mut calls = Vec::new();

        let result = pick_directory_with(
            |program, args| {
                calls.push((program.to_string(), args.to_vec()));
                Ok(picker_output(true, &format!("  {selected}\n"), ""))
            },
            |path| path.is_dir(),
        )
        .unwrap();

        assert_eq!(result, selected);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "zenity");
        assert!(calls[0].1.iter().any(|arg| arg == "--directory"));
    }

    #[test]
    fn directory_picker_falls_back_when_a_program_is_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let selected = tmp.path().to_string_lossy().into_owned();
        let mut calls = Vec::new();

        let result = pick_directory_with(
            |program, _args| {
                calls.push(program.to_string());
                if program == "zenity" {
                    Err(DirectoryPickerRunError::NotFound)
                } else {
                    Ok(picker_output(true, &selected, ""))
                }
            },
            |path| path.is_dir(),
        )
        .unwrap();

        assert_eq!(result, selected);
        assert_eq!(calls, ["zenity", "kdialog"]);
    }

    #[test]
    fn directory_picker_cancel_returns_empty_without_opening_another() {
        let mut calls = 0;

        let result = pick_directory_with(
            |_program, _args| {
                calls += 1;
                Ok(picker_output(false, "", ""))
            },
            |_path| true,
        )
        .unwrap();

        assert_eq!(result, "");
        assert_eq!(calls, 1);
    }

    #[test]
    fn directory_picker_rejects_a_successful_non_directory_result() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("wallpaper.jpg");
        std::fs::write(&file, b"wallpaper").unwrap();

        let error = pick_directory_with(
            |_program, _args| Ok(picker_output(true, &file.to_string_lossy(), "")),
            |path| path.is_dir(),
        )
        .unwrap_err();

        assert!(error.contains("not an existing directory"), "{error}");
    }

    #[test]
    fn directory_picker_explains_when_no_supported_program_is_installed() {
        let error = pick_directory_with(
            |_program, _args| Err(DirectoryPickerRunError::NotFound),
            |_path| true,
        )
        .unwrap_err();

        assert!(error.contains("zenity"), "{error}");
        assert!(error.contains("kdialog"), "{error}");
        assert!(error.contains("yad"), "{error}");
    }

    #[test]
    fn open_location_target_dir_returns_self() {
        let tmp = tempfile::tempdir().unwrap();
        let result = open_location_target(&tmp.path().to_string_lossy()).unwrap();
        assert_eq!(result, tmp.path());
    }

    #[test]
    fn open_location_target_file_returns_parent() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("test.txt");
        std::fs::write(&f, b"hello").unwrap();
        let result = open_location_target(&f.to_string_lossy()).unwrap();
        assert_eq!(result, tmp.path());
    }

    #[test]
    fn open_location_target_regular_wallpaper_file_returns_containing_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("wallpapers");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("wall.png");
        std::fs::write(&file, b"png").unwrap();

        let result = open_location_target(&file.to_string_lossy()).unwrap();
        assert_eq!(result, dir);
    }

    #[test]
    fn file_manager_auto_candidates_excludes_xdg_open_and_terminals() {
        let candidates = file_manager_candidates("auto");
        assert!(candidates.contains(&"nautilus"));
        assert!(candidates.contains(&"dolphin"));
        assert!(candidates.contains(&"thunar"));
        assert!(candidates.contains(&"nemo"));
        assert!(candidates.contains(&"pcmanfm"));
        assert!(!candidates.contains(&"xdg-open"));
        assert!(!candidates.contains(&"yazi"));
        assert!(!candidates.contains(&"kitty"));
        assert!(!candidates.contains(&"alacritty"));
        assert!(!candidates.contains(&"foot"));
        assert!(!candidates.contains(&"konsole"));
        assert!(!candidates.contains(&"gnome-terminal"));
        assert_eq!(candidates.len(), 5);
    }

    #[test]
    fn file_manager_specific_returns_single_candidate() {
        let candidates = file_manager_candidates("nautilus");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0], "nautilus");
    }

    #[test]
    fn file_manager_custom_returns_empty() {
        let candidates = file_manager_candidates("custom");
        assert!(candidates.is_empty());
    }

    #[test]
    fn custom_command_parts_appends_target() {
        let parts = custom_command_parts("nautilus", "/tmp/walls").unwrap();
        assert_eq!(parts, vec!["nautilus", "/tmp/walls"]);
    }

    #[test]
    fn custom_command_parts_replaces_path_placeholder() {
        let parts = custom_command_parts("nautilus {path}", "/tmp/walls").unwrap();
        assert_eq!(parts, vec!["nautilus", "/tmp/walls"]);
    }

    #[test]
    fn custom_command_parts_empty_errors() {
        assert!(custom_command_parts("", "/tmp/walls").is_err());
        assert!(custom_command_parts("   ", "/tmp/walls").is_err());
    }

    #[test]
    fn terminal_candidates_keeps_env_and_fallbacks() {
        let candidates = terminal_candidates(Some("ghostty --foo"));
        assert_eq!(candidates[0], "ghostty");
        assert!(candidates.contains(&"kitty"));
        assert!(candidates.contains(&"alacritty"));
        assert!(candidates.contains(&"konsole"));
        assert_eq!(candidates.len(), 7); // ghostty + 6 fallbacks
    }

    #[test]
    fn terminal_candidates_deduplicates_env() {
        let candidates = terminal_candidates(Some("kitty"));
        let kitty_count = candidates.iter().filter(|&&c| c == "kitty").count();
        assert_eq!(kitty_count, 1);
        assert_eq!(candidates.len(), 6);
    }

    #[test]
    fn terminal_spawn_command_kitty() {
        let inner: Vec<String> = vec!["yazi".into(), "/tmp/walls".into()];
        let (prog, args) = terminal_spawn_command("kitty", &inner).unwrap();
        assert_eq!(prog, "kitty");
        assert_eq!(args, vec!["--", "yazi", "/tmp/walls"]);
    }

    #[test]
    fn terminal_spawn_command_alacritty() {
        let inner: Vec<String> = vec!["yazi".into(), "/tmp/walls".into()];
        let (_prog, args) = terminal_spawn_command("alacritty", &inner).unwrap();
        assert_eq!(args, vec!["-e", "yazi", "/tmp/walls"]);
    }

    #[test]
    fn terminal_spawn_command_foot() {
        let inner: Vec<String> = vec!["yazi".into(), "/tmp/walls".into()];
        let (_prog, args) = terminal_spawn_command("foot", &inner).unwrap();
        assert_eq!(args, vec!["yazi", "/tmp/walls"]);
    }

    #[test]
    fn terminal_spawn_command_wezterm() {
        let inner: Vec<String> = vec!["yazi".into(), "/tmp/walls".into()];
        let (_prog, args) = terminal_spawn_command("wezterm", &inner).unwrap();
        assert_eq!(args, vec!["start", "--", "yazi", "/tmp/walls"]);
    }

    #[test]
    fn terminal_spawn_command_gnome_terminal() {
        let inner: Vec<String> = vec!["yazi".into(), "/tmp/walls".into()];
        let (_prog, args) = terminal_spawn_command("gnome-terminal", &inner).unwrap();
        assert_eq!(args, vec!["--", "yazi", "/tmp/walls"]);
    }

    #[test]
    fn terminal_spawn_command_konsole() {
        let inner: Vec<String> = vec!["yazi".into(), "/tmp/walls".into()];
        let (_prog, args) = terminal_spawn_command("konsole", &inner).unwrap();
        assert_eq!(args, vec!["-e", "yazi", "/tmp/walls"]);
    }

    #[test]
    fn terminal_spawn_command_unknown_returns_none() {
        let inner: Vec<String> = vec!["yazi".into()];
        assert!(terminal_spawn_command("unknown", &inner).is_none());
    }

    #[test]
    fn terminal_file_manager_yazi_includes_target() {
        let cmd = terminal_file_manager_command("yazi", "", "/tmp/walls").unwrap();
        assert_eq!(cmd, vec!["yazi", "/tmp/walls"]);
    }

    #[test]
    fn open_with_custom_cmd_empty_errors() {
        let target = std::path::Path::new("/tmp/test");
        let err = open_with_file_manager(target, "custom", "");
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("empty"));
    }

    #[test]
    fn try_terminal_spawn_first_fails_second_succeeds() {
        let candidates = vec!["alacritty", "foot"];
        let tui_cmd: Vec<String> = vec!["yazi".into(), "/tmp".into()];
        let mut tries: Vec<String> = Vec::new();
        let result = try_terminal_spawn_candidates(
            &candidates,
            &tui_cmd,
            "yazi",
            "/tmp",
            &mut |prog: &str, _: &[String]| {
                tries.push(prog.to_string());
                prog != "alacritty"
            },
        );
        assert!(result.is_ok());
        assert_eq!(tries, vec!["alacritty", "foot"]);
    }

    #[test]
    fn try_terminal_spawn_all_fail_reports_attempted() {
        let candidates = vec!["alacritty", "kitty"];
        let tui_cmd: Vec<String> = vec!["yazi".into(), "/tmp".into()];
        let result =
            try_terminal_spawn_candidates(&candidates, &tui_cmd, "yazi", "/tmp", &mut |_, _| false);
        let err = result.unwrap_err();
        assert!(err.contains("Tried:"));
        assert!(err.contains("alacritty"));
        assert!(err.contains("kitty"));
    }

    #[test]
    fn try_terminal_spawn_skips_unknown_and_continues() {
        let candidates = vec!["unknown", "kitty"];
        let tui_cmd: Vec<String> = vec!["yazi".into(), "/tmp".into()];
        let mut tries: Vec<String> = Vec::new();
        let result = try_terminal_spawn_candidates(
            &candidates,
            &tui_cmd,
            "yazi",
            "/tmp",
            &mut |prog: &str, _: &[String]| {
                tries.push(prog.to_string());
                true
            },
        );
        assert!(result.is_ok());
        assert_eq!(tries, vec!["kitty"]);
    }
}
