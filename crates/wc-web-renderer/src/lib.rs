use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::Parser;

#[derive(Debug, Clone, Parser)]
pub struct RendererArgs {
    #[arg(long)]
    pub project: PathBuf,
    #[arg(long)]
    pub file: Option<String>,
    #[arg(long, default_value_t = 1920)]
    pub width: i32,
    #[arg(long, default_value_t = 1080)]
    pub height: i32,
    #[arg(long)]
    pub output: Option<String>,
    #[arg(long, default_value = "on")]
    pub audio: String,
    #[arg(long)]
    pub debug: bool,
    #[arg(long)]
    pub dump_spec: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderSpec {
    pub project_dir: PathBuf,
    pub html_file: PathBuf,
    pub file_uri: String,
    pub width: i32,
    pub height: i32,
    pub output: Option<String>,
    pub audio: bool,
    pub debug: bool,
}

pub fn render_spec_from_args(args: &RendererArgs) -> Result<RenderSpec> {
    let project_dir = args
        .project
        .canonicalize()
        .with_context(|| format!("cannot canonicalize project: {}", args.project.display()))?;
    if !project_dir.is_dir() {
        bail!("project is not a directory: {}", project_dir.display());
    }

    let rel = match args.file.as_deref() {
        Some(file) if !file.trim().is_empty() => file.trim().to_string(),
        _ => project_file_from_json(&project_dir)?,
    };
    let html_file = resolve_project_file(&project_dir, &rel)?;
    Ok(RenderSpec {
        file_uri: file_uri(&html_file),
        project_dir,
        html_file,
        width: args.width.max(1),
        height: args.height.max(1),
        output: args
            .output
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        audio: args.audio != "off",
        debug: args.debug,
    })
}

pub fn project_file_from_json(project_dir: &Path) -> Result<String> {
    let project_json = project_dir.join("project.json");
    let content = std::fs::read_to_string(&project_json).with_context(|| {
        format!(
            "project.json missing or unreadable: {}",
            project_json.display()
        )
    })?;
    let value: serde_json::Value = serde_json::from_str(&content)
        .with_context(|| format!("project.json invalid: {}", project_json.display()))?;
    let ty = value
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if ty != "web" {
        bail!("project is not a Web Wallpaper Engine project: {}", ty);
    }
    Ok(value
        .get("file")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("index.html")
        .to_string())
}

pub fn resolve_project_file(project_dir: &Path, rel: &str) -> Result<PathBuf> {
    let rel_path = Path::new(rel);
    for comp in rel_path.components() {
        match comp {
            Component::ParentDir => bail!("project file path traversal rejected: {}", rel),
            Component::RootDir | Component::Prefix(_) => {
                bail!("project file absolute path rejected: {}", rel)
            }
            _ => {}
        }
    }

    let root = project_dir
        .canonicalize()
        .with_context(|| format!("cannot canonicalize project: {}", project_dir.display()))?;
    let candidate = root.join(rel_path);
    let canonical = candidate
        .canonicalize()
        .with_context(|| format!("cannot canonicalize project file: {}", candidate.display()))?;
    if !canonical.starts_with(&root) {
        bail!("project file escapes project directory: {}", rel);
    }
    if !canonical.is_file() {
        bail!(
            "project file is not a regular file: {}",
            canonical.display()
        );
    }
    Ok(canonical)
}

pub fn file_uri(path: &Path) -> String {
    let mut encoded = String::from("file://");
    for b in path.to_string_lossy().as_bytes() {
        let ch = *b as char;
        if ch.is_ascii_alphanumeric() || matches!(ch, '/' | '-' | '_' | '.' | '~') {
            encoded.push(ch);
        } else {
            encoded.push_str(&format!("%{:02X}", b));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    fn web_project() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("index.html"), "<html></html>").unwrap();
        std::fs::write(
            tmp.path().join("project.json"),
            r#"{"type":"Web","file":"index.html"}"#,
        )
        .unwrap();
        tmp
    }

    #[test]
    fn project_json_defaults_to_index_html() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("index.html"), "").unwrap();
        std::fs::write(tmp.path().join("project.json"), r#"{"type":"web"}"#).unwrap();
        assert_eq!(project_file_from_json(tmp.path()).unwrap(), "index.html");
    }

    #[test]
    fn project_json_rejects_non_web_type() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("project.json"), r#"{"type":"scene"}"#).unwrap();
        assert!(project_file_from_json(tmp.path()).is_err());
    }

    #[test]
    fn resolve_rejects_absolute_path() {
        let tmp = web_project();
        assert!(resolve_project_file(tmp.path(), "/etc/passwd").is_err());
    }

    #[test]
    fn resolve_rejects_traversal() {
        let tmp = web_project();
        assert!(resolve_project_file(tmp.path(), "../index.html").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn resolve_rejects_symlink_escape() {
        use std::os::unix::fs::symlink;
        let tmp = web_project();
        let outside = tempfile::NamedTempFile::new().unwrap();
        symlink(outside.path(), tmp.path().join("escape.html")).unwrap();
        assert!(resolve_project_file(tmp.path(), "escape.html").is_err());
    }

    #[test]
    fn render_spec_uses_project_file_and_encodes_uri() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a b.html"), "").unwrap();
        std::fs::write(
            tmp.path().join("project.json"),
            r#"{"type":"web","file":"a b.html"}"#,
        )
        .unwrap();
        let args = RendererArgs {
            project: tmp.path().to_path_buf(),
            file: None,
            width: 1280,
            height: 720,
            output: Some("eDP-1".into()),
            audio: "off".into(),
            debug: true,
            dump_spec: false,
        };
        let spec = render_spec_from_args(&args).unwrap();
        assert!(spec.file_uri.contains("a%20b.html"));
        assert!(!spec.audio);
        assert_eq!(spec.output.as_deref(), Some("eDP-1"));
    }
}
