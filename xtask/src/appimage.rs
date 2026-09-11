//! AppImage runtime integrity and host-first AppDir preparation.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{is_lower_hex, resolve_from};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ReleasePrepareAppImageArgs {
    pub(super) appdir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ReleaseVerifyAppImageRuntimeArgs {
    pub(super) runtime: PathBuf,
    pub(super) appimage: PathBuf,
    pub(super) runtime_size: usize,
    pub(super) expected_sha256: String,
}

fn elf_u16(bytes: &[u8], offset: usize) -> Result<u16, String> {
    let value = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| "ELF header is truncated".to_string())?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

fn elf_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let value = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| "ELF section header is truncated".to_string())?;
    Ok(u32::from_le_bytes(value.try_into().unwrap()))
}

fn elf_u64(bytes: &[u8], offset: usize) -> Result<u64, String> {
    let value = bytes
        .get(offset..offset + 8)
        .ok_or_else(|| "ELF section header is truncated".to_string())?;
    Ok(u64::from_le_bytes(value.try_into().unwrap()))
}

fn appimage_digest_range(runtime: &[u8]) -> Result<std::ops::Range<usize>, String> {
    if runtime.get(..7) != Some(&[0x7f, b'E', b'L', b'F', 2, 1, 1]) {
        return Err("pinned runtime is not a little-endian ELF64 file".to_string());
    }
    let section_offset = usize::try_from(elf_u64(runtime, 0x28)?)
        .map_err(|_| "ELF section table offset is too large".to_string())?;
    let section_size = usize::from(elf_u16(runtime, 0x3a)?);
    let section_count = usize::from(elf_u16(runtime, 0x3c)?);
    let names_index = usize::from(elf_u16(runtime, 0x3e)?);
    if section_size < 64 || section_count == 0 || names_index >= section_count {
        return Err("ELF section table is invalid".to_string());
    }
    let section = |index: usize| -> Result<&[u8], String> {
        let start = section_offset
            .checked_add(
                index
                    .checked_mul(section_size)
                    .ok_or("ELF section overflow")?,
            )
            .ok_or("ELF section overflow")?;
        let end = start
            .checked_add(section_size)
            .ok_or("ELF section overflow")?;
        runtime
            .get(start..end)
            .ok_or_else(|| "ELF section table is truncated".to_string())
    };
    let names = section(names_index)?;
    let names_offset = usize::try_from(elf_u64(names, 24)?)
        .map_err(|_| "ELF name table offset is too large".to_string())?;
    let names_size = usize::try_from(elf_u64(names, 32)?)
        .map_err(|_| "ELF name table size is too large".to_string())?;
    let names_end = names_offset
        .checked_add(names_size)
        .ok_or("ELF name table overflow")?;
    let names = runtime
        .get(names_offset..names_end)
        .ok_or_else(|| "ELF name table is truncated".to_string())?;

    for index in 0..section_count {
        let header = section(index)?;
        let name_offset = usize::try_from(elf_u32(header, 0)?)
            .map_err(|_| "ELF section name offset is too large".to_string())?;
        let name_bytes = names.get(name_offset..).unwrap_or_default();
        let name_end = name_bytes
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(name_bytes.len());
        if &name_bytes[..name_end] == b".digest_md5" {
            let offset = usize::try_from(elf_u64(header, 24)?)
                .map_err(|_| "digest section offset is too large".to_string())?;
            let size = usize::try_from(elf_u64(header, 32)?)
                .map_err(|_| "digest section size is too large".to_string())?;
            if size != 16
                || offset
                    .checked_add(size)
                    .is_none_or(|end| end > runtime.len())
            {
                return Err("invalid .digest_md5 section bounds".to_string());
            }
            return Ok(offset..offset + size);
        }
    }
    Err("pinned runtime has no .digest_md5 section".to_string())
}

pub(super) fn verify_appimage_runtime(
    repo_root: &Path,
    args: &ReleaseVerifyAppImageRuntimeArgs,
) -> Result<(), String> {
    if !is_lower_hex(&args.expected_sha256, 64) {
        return Err("expected runtime SHA-256 must be lowercase hex".to_string());
    }
    let runtime_path = resolve_from(repo_root, &args.runtime);
    let appimage_path = resolve_from(repo_root, &args.appimage);
    let runtime = std::fs::read(&runtime_path)
        .map_err(|error| format!("failed to read {}: {error}", runtime_path.display()))?;
    if runtime.len() != args.runtime_size {
        return Err(format!(
            "pinned runtime size is {}, expected {}",
            runtime.len(),
            args.runtime_size
        ));
    }
    let checksum = Command::new("sha256sum")
        .arg(&runtime_path)
        .output()
        .map_err(|error| format!("failed to run sha256sum: {error}"))?;
    if !checksum.status.success() {
        return Err("sha256sum failed for pinned runtime".to_string());
    }
    let actual_sha256 = String::from_utf8_lossy(&checksum.stdout)
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string();
    if actual_sha256 != args.expected_sha256 {
        return Err(format!(
            "pinned runtime SHA-256 is {actual_sha256}, expected {}",
            args.expected_sha256
        ));
    }

    let digest = appimage_digest_range(&runtime)?;
    let file = std::fs::File::open(&appimage_path)
        .map_err(|error| format!("failed to open {}: {error}", appimage_path.display()))?;
    let mut embedded = Vec::with_capacity(args.runtime_size);
    file.take(args.runtime_size as u64)
        .read_to_end(&mut embedded)
        .map_err(|error| format!("failed to read AppImage runtime: {error}"))?;
    if embedded.len() != args.runtime_size {
        return Err("AppImage is shorter than its pinned runtime".to_string());
    }
    if embedded[..digest.start] != runtime[..digest.start]
        || embedded[digest.end..] != runtime[digest.end..]
    {
        return Err("AppImage runtime differs outside .digest_md5".to_string());
    }
    Ok(())
}

const APPIMAGE_APP_RUN: &str = r#"#!/bin/sh
set -eu
APPDIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
export APPDIR

host_library_path=
host_webkit_available=
for directory in /usr/lib/x86_64-linux-gnu /lib/x86_64-linux-gnu /usr/lib64 /usr/lib /lib64 /lib; do
  if [ -d "$directory" ]; then
    host_library_path="${host_library_path:+$host_library_path:}$directory"
    if [ -e "$directory/libwebkit2gtk-4.1.so.0" ]; then
      host_webkit_available=1
    fi
  fi
done
if [ "${WCR_APPIMAGE_FORCE_BUNDLED:-0}" != "1" ] && [ -n "$host_library_path" ] && [ -n "$host_webkit_available" ]; then
  export LD_LIBRARY_PATH="$host_library_path${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
else
  bundled_library_path="$APPDIR/usr/lib:$APPDIR/usr/lib/x86_64-linux-gnu"
  export LD_LIBRARY_PATH="$bundled_library_path${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
  export GTK_DATA_PREFIX="$APPDIR"
  export XDG_DATA_DIRS="$APPDIR/usr/share${XDG_DATA_DIRS:+:$XDG_DATA_DIRS}"
  export GSETTINGS_SCHEMA_DIR="$APPDIR/usr/share/glib-2.0/schemas"
  export GTK_EXE_PREFIX="$APPDIR/usr"
  export GTK_PATH="$APPDIR/usr/lib/x86_64-linux-gnu/gtk-3.0"
  export GTK_IM_MODULE_FILE="$APPDIR/usr/lib/x86_64-linux-gnu/gtk-3.0/3.0.0/immodules.cache"
  export GDK_PIXBUF_MODULE_FILE="$APPDIR/usr/lib/x86_64-linux-gnu/gdk-pixbuf-2.0/2.10.0/loaders.cache"
  export GIO_EXTRA_MODULES="$APPDIR/usr/lib/x86_64-linux-gnu/gio/modules"
  cd "$APPDIR/usr"
fi

if [ "${WCR_WEBKIT_DISABLE_DMABUF_RENDERER:-0}" = "1" ] && [ -z "${WEBKIT_DISABLE_DMABUF_RENDERER+x}" ]; then
  export WEBKIT_DISABLE_DMABUF_RENDERER=1
fi
exec "$APPDIR/usr/bin/wallpaper-console-tauri" "$@"
"#;

pub(super) fn prepare_appimage_appdir(appdir: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    if !appdir.is_dir() {
        return Err(format!("AppDir is missing: {}", appdir.display()));
    }
    let gui = appdir.join("usr/bin/wallpaper-console-tauri");
    if !gui.is_file() {
        return Err(format!("AppDir GUI binary is missing: {}", gui.display()));
    }

    let hooks = appdir.join("apprun-hooks");
    if hooks.exists() {
        std::fs::remove_dir_all(&hooks)
            .map_err(|error| format!("failed to remove {}: {error}", hooks.display()))?;
    }
    for relative in ["AppRun", "AppRun.wrapped"] {
        let path = appdir.join(relative);
        if std::fs::symlink_metadata(&path).is_ok() {
            std::fs::remove_file(&path)
                .map_err(|error| format!("failed to remove {}: {error}", path.display()))?;
        }
    }

    let app_run = appdir.join("AppRun");
    std::fs::write(&app_run, APPIMAGE_APP_RUN)
        .map_err(|error| format!("failed to write {}: {error}", app_run.display()))?;
    let mut permissions = std::fs::metadata(&app_run)
        .map_err(|error| format!("failed to stat {}: {error}", app_run.display()))?
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&app_run, permissions)
        .map_err(|error| format!("failed to make {} executable: {error}", app_run.display()))?;

    println!(
        "prepared host-first AppImage AppDir at {}",
        appdir.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepares_host_first_appdir_with_bundled_fallback() {
        use std::os::unix::fs::PermissionsExt;

        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wallpaper-console-appdir-test-{}-{unique}",
            std::process::id()
        ));
        let appdir = root.join("AppDir");
        let gui = appdir.join("usr/bin/wallpaper-console-tauri");
        std::fs::create_dir_all(gui.parent().unwrap()).unwrap();
        std::fs::create_dir_all(appdir.join("usr/lib")).unwrap();
        std::fs::create_dir_all(appdir.join("usr/share/wallpaper-console")).unwrap();
        std::fs::create_dir_all(appdir.join("apprun-hooks")).unwrap();
        std::fs::write(&gui, b"synthetic gui binary").unwrap();
        std::fs::write(appdir.join("usr/lib/libwebkit2gtk-4.1.so.0"), b"bundled").unwrap();
        std::fs::write(appdir.join("usr/share/wallpaper-console/keep"), b"resource").unwrap();
        std::fs::write(appdir.join("AppRun.wrapped"), b"bundled launcher").unwrap();
        std::fs::write(
            appdir.join("apprun-hooks/linuxdeploy-plugin-gtk.sh"),
            b"hook",
        )
        .unwrap();

        prepare_appimage_appdir(&appdir).unwrap();

        let app_run = appdir.join("AppRun");
        let launcher = std::fs::read_to_string(&app_run).unwrap();
        assert!(launcher.starts_with("#!/bin/sh\n"));
        assert!(launcher.contains("WCR_WEBKIT_DISABLE_DMABUF_RENDERER"));
        assert!(launcher.contains("WEBKIT_DISABLE_DMABUF_RENDERER=1"));
        assert!(launcher.contains(r#"exec "$APPDIR/usr/bin/wallpaper-console-tauri" "$@""#));
        assert!(!launcher.contains("AppRun.wrapped"));
        assert!(!launcher.contains("GDK_BACKEND"));
        assert!(launcher.contains("/usr/lib"));
        assert!(launcher.contains("LD_LIBRARY_PATH"));
        assert!(launcher.contains("host_webkit_available"));
        assert!(launcher.contains("libwebkit2gtk-4.1.so.0"));
        assert_ne!(
            std::fs::metadata(&app_run).unwrap().permissions().mode() & 0o111,
            0
        );
        assert!(appdir.join("usr/lib/libwebkit2gtk-4.1.so.0").is_file());
        assert!(!appdir.join("AppRun.wrapped").exists());
        assert!(!appdir.join("apprun-hooks").exists());
        assert!(appdir.join("usr/share/wallpaper-console/keep").is_file());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bundled_appimage_fallback_initializes_loader_and_runtime_resources() {
        use std::os::unix::fs::PermissionsExt;

        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wallpaper-console-fallback-test-{}-{unique}",
            std::process::id()
        ));
        let appdir = root.join("AppDir");
        let gui = appdir.join("usr/bin/wallpaper-console-tauri");
        let outside = root.join("outside");
        std::fs::create_dir_all(gui.parent().unwrap()).unwrap();
        std::fs::create_dir_all(appdir.join("usr/lib/x86_64-linux-gnu")).unwrap();
        std::fs::create_dir_all(appdir.join("usr/share/glib-2.0/schemas")).unwrap();
        std::fs::create_dir_all(appdir.join("test-host")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(
            &gui,
            b"#!/bin/sh\nprintf '%s\n' \"$PWD\" \"${LD_LIBRARY_PATH:-}\" \"${GSETTINGS_SCHEMA_DIR:-}\" \"${GDK_PIXBUF_MODULE_FILE:-}\" \"${GIO_EXTRA_MODULES:-}\"\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&gui).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&gui, permissions).unwrap();
        prepare_appimage_appdir(&appdir).unwrap();

        let app_run = appdir.join("AppRun");
        let output = Command::new(&app_run)
            .current_dir(&outside)
            .env("WCR_APPIMAGE_FORCE_BUNDLED", "1")
            .env("LD_LIBRARY_PATH", "/caller/lib")
            .env_remove("GSETTINGS_SCHEMA_DIR")
            .env_remove("GDK_PIXBUF_MODULE_FILE")
            .env_remove("GIO_EXTRA_MODULES")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let values: Vec<&str> = stdout.lines().collect();
        assert_eq!(values[0], appdir.join("usr").to_string_lossy());
        assert_eq!(
            values[1],
            format!(
                "{}/usr/lib:{}/usr/lib/x86_64-linux-gnu:/caller/lib",
                appdir.display(),
                appdir.display()
            )
        );
        assert_eq!(
            values[2],
            appdir.join("usr/share/glib-2.0/schemas").to_string_lossy()
        );
        assert_eq!(
            values[3],
            appdir
                .join("usr/lib/x86_64-linux-gnu/gdk-pixbuf-2.0/2.10.0/loaders.cache")
                .to_string_lossy()
        );
        assert_eq!(
            values[4],
            appdir
                .join("usr/lib/x86_64-linux-gnu/gio/modules")
                .to_string_lossy()
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn appimage_runtime_verifier_ignores_only_payload_digest() {
        fn u16_at(bytes: &mut [u8], offset: usize, value: u16) {
            bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
        fn u32_at(bytes: &mut [u8], offset: usize, value: u32) {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        fn u64_at(bytes: &mut [u8], offset: usize, value: u64) {
            bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("wcr-runtime-test-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let runtime_path = root.join("runtime");
        let appimage_path = root.join("candidate.AppImage");
        let mut runtime = vec![0u8; 0x210];
        runtime[..16]
            .copy_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        u16_at(&mut runtime, 0x10, 2);
        u16_at(&mut runtime, 0x12, 62);
        u32_at(&mut runtime, 0x14, 1);
        u64_at(&mut runtime, 0x28, 0x100);
        u16_at(&mut runtime, 0x34, 64);
        u16_at(&mut runtime, 0x3a, 64);
        u16_at(&mut runtime, 0x3c, 3);
        u16_at(&mut runtime, 0x3e, 1);
        u32_at(&mut runtime, 0x140, 1);
        u32_at(&mut runtime, 0x144, 3);
        u64_at(&mut runtime, 0x158, 0x1c0);
        u64_at(&mut runtime, 0x160, 23);
        u32_at(&mut runtime, 0x180, 11);
        u32_at(&mut runtime, 0x184, 1);
        u64_at(&mut runtime, 0x198, 0x200);
        u64_at(&mut runtime, 0x1a0, 16);
        runtime[0x1c0..0x1d7].copy_from_slice(b"\x00.shstrtab\x00.digest_md5\x00");
        runtime[0x200..0x210].copy_from_slice(b"original-digest!");
        std::fs::write(&runtime_path, &runtime).unwrap();
        let output = Command::new("sha256sum")
            .arg(&runtime_path)
            .output()
            .unwrap();
        let sha256 = String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .next()
            .unwrap()
            .to_string();
        let args = ReleaseVerifyAppImageRuntimeArgs {
            runtime: runtime_path,
            appimage: appimage_path.clone(),
            runtime_size: runtime.len(),
            expected_sha256: sha256,
        };
        let mut candidate = runtime.clone();
        candidate[0x200..0x210].copy_from_slice(b"payload-digest!!");
        candidate.extend_from_slice(b"squashfs payload");
        std::fs::write(&appimage_path, &candidate).unwrap();
        verify_appimage_runtime(Path::new("/"), &args).unwrap();
        candidate[0xd0] = 1;
        std::fs::write(&appimage_path, &candidate).unwrap();
        assert!(verify_appimage_runtime(Path::new("/"), &args).is_err());

        let mut malformed_sections = runtime.clone();
        u64_at(&mut malformed_sections, 0x28, u64::MAX);
        assert!(appimage_digest_range(&malformed_sections).is_err());
        let mut malformed_names = runtime;
        u64_at(&mut malformed_names, 0x158, u64::MAX);
        assert!(appimage_digest_range(&malformed_names).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
