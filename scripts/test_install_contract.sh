#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

fake_gui="$tmp/wallpaper-console-tauri"
fake_cli="$tmp/wallpaper-console-rust"
fake_dist="$tmp/dist"
prefix="$tmp/prefix with spaces % and \$"

expect_failure() {
  local expected="$1"
  shift
  local output
  if output="$("$@" 2>&1)"; then
    echo "FAIL: command unexpectedly succeeded: $*" >&2
    exit 1
  fi
  grep -Fq -- "$expected" <<<"$output" || {
    echo "FAIL: expected error containing '$expected', got:" >&2
    printf '%s\n' "$output" >&2
    exit 1
  }
}

install_fake() {
  local install_prefix="$1"
  shift
  WCR_INSTALL_SKIP_BUILD=1 \
  WCR_INSTALL_TAURI_BIN="$fake_gui" \
  WCR_INSTALL_CLI_BIN="$fake_cli" \
  WCR_INSTALL_FRONTEND_DIST="$fake_dist" \
    "$ROOT/install.sh" --prefix "$install_prefix" "$@"
}

assert_no_staging_tree() {
  local install_prefix="$1"
  if compgen -G "$install_prefix/.wallpaper-console-rust.install.*" >/dev/null; then
    echo "FAIL: installer left a staging tree under $install_prefix" >&2
    find "$install_prefix" -maxdepth 1 -name '.wallpaper-console-rust.install.*' -print >&2
    exit 1
  fi
  if [[ -e "$install_prefix/.wallpaper-console-rust.install.lock" \
    || -L "$install_prefix/.wallpaper-console-rust.install.lock" ]]; then
    echo "FAIL: installer left a lock directory under $install_prefix" >&2
    exit 1
  fi
}

"$ROOT/install.sh" --help | grep -q 'Wallpaper Console installer'
"$ROOT/install.sh" --help | grep -q -- '--build-only'
"$ROOT/install.sh" --help | grep -q -- '--uninstall'
"$ROOT/install.sh" --help | grep -q -- '--prefix DIR'
"$ROOT/install.sh" --help | grep -q -- '--force'
grep -Fq 'cargo build --locked --release' "$ROOT/install.sh"
grep -Fq 'Install zenity, kdialog, or yad' "$ROOT/install.sh"
expect_failure 'Unknown option: --unknown' "$ROOT/install.sh" --unknown
expect_failure '--prefix must be an absolute path' \
  "$ROOT/install.sh" --prefix relative-prefix --uninstall
expect_failure '--prefix must not contain newline characters' \
  "$ROOT/install.sh" --prefix "$tmp/prefix"$'\n'"newline" --uninstall

mkdir -p "$fake_dist"
# shellcheck disable=SC2016 # Write runtime expansions into the fake executable.
printf '#!/bin/sh\nprintf "gui %%s dmabuf=%%s\\n" "$*" "${WEBKIT_DISABLE_DMABUF_RENDERER-unset}"\n' >"$fake_gui"
printf '#!/bin/sh\necho cli "$@"\n' >"$fake_cli"
chmod +x "$fake_gui" "$fake_cli"
printf '<!doctype html><div id="root"></div>\n' >"$fake_dist/index.html"

backslash_prefix="$tmp/prefix\\with-backslash"
expect_failure '--prefix must not contain backslash characters' \
  install_fake "$backslash_prefix"
test ! -e "$backslash_prefix"

empty_prefix="$tmp/empty-uninstall-prefix"
"$ROOT/install.sh" --prefix "$empty_prefix" --uninstall >/dev/null
test ! -e "$empty_prefix"

locked_prefix="$tmp/concurrent-lock-prefix"
mkdir -p "$locked_prefix/.wallpaper-console-rust.install.lock"
printf '%s\n' "$$" >"$locked_prefix/.wallpaper-console-rust.install.lock/owner-pid"
expect_failure 'Another install or uninstall is already active' \
  install_fake "$locked_prefix"
rm -f "$locked_prefix/.wallpaper-console-rust.install.lock/owner-pid"
rmdir "$locked_prefix/.wallpaper-console-rust.install.lock"

mkdir -p "$prefix/bin"
printf 'legacy-cli-must-survive-refusal\n' >"$prefix/bin/wallpaper-console-rust"
expect_failure 'Existing untracked install files would be overwritten' \
  install_fake "$prefix"
grep -Fq 'legacy-cli-must-survive-refusal' "$prefix/bin/wallpaper-console-rust"
test ! -e "$prefix/share/wallpaper-console-rust/install-manifest-v1"
assert_no_staging_tree "$prefix"

untracked_uninstall_output="$("$ROOT/install.sh" --prefix "$prefix" --uninstall 2>&1)"
grep -Fq 'No ownership manifest found; existing files were preserved' \
  <<<"$untracked_uninstall_output"
grep -Fq 'legacy-cli-must-survive-refusal' "$prefix/bin/wallpaper-console-rust"

install_fake "$prefix" --force
assert_no_staging_tree "$prefix"

gui_wrapper="$prefix/bin/wallpaper-console-gui-rust"
gui_bin="$prefix/lib/wallpaper-console-rust/wallpaper-console-gui-rust"
cli_bin="$prefix/bin/wallpaper-console-rust"
desktop="$prefix/share/applications/wallpaper-console-gui-rust.desktop"
icon="$prefix/share/icons/hicolor/128x128/apps/wallpaper-console-gui-rust.png"
license="$prefix/share/licenses/wallpaper-console-rust/LICENSE"
manifest="$prefix/share/wallpaper-console-rust/install-manifest-v1"

test -x "$gui_wrapper"
test -x "$gui_bin"
test -x "$cli_bin"
test -f "$desktop"
test -f "$icon"
test -f "$license"
test -f "$manifest"

test "$(stat -c '%a' "$gui_wrapper")" = 755
test "$(stat -c '%a' "$gui_bin")" = 755
test "$(stat -c '%a' "$cli_bin")" = 755
test "$(stat -c '%a' "$desktop")" = 644
test "$(stat -c '%a' "$icon")" = 644
test "$(stat -c '%a' "$license")" = 644
test "$(stat -c '%a' "$manifest")" = 644

# shellcheck disable=SC2016 # Assert the literal runtime expansion in the wrapper.
grep -Fq 'install_prefix=$(CDPATH= cd -- "$launcher_dir/.." && pwd)' "$gui_wrapper"
desktop_exec="${gui_wrapper//%/%%}"
desktop_exec="${desktop_exec//\$/\\\$}"
grep -Fq "Exec=\"$desktop_exec\"" "$desktop"
grep -Fq "TryExec=$gui_wrapper" "$desktop"
grep -Fq 'StartupNotify=true' "$desktop"
grep -Fq 'Categories=Graphics;' "$desktop"
grep -Fq 'Keywords=Wallpaper;Background;Wayland;' "$desktop"
cmp -s "$ROOT/apps/tauri-gui/src-tauri/icons/128x128.png" "$icon"
cmp -s "$ROOT/LICENSE" "$license"
test "$(head -n 1 "$manifest")" = 'wallpaper-console-rust-install-manifest-v1'
test "$(wc -l <"$manifest")" = 7
for owned in "$gui_wrapper" "$gui_bin" "$cli_bin" "$desktop" "$icon" "$license"; do
  owned_hash="$(sha256sum "$owned" | awk '{print $1}')"
  owned_relative="${owned#"$prefix/"}"
  grep -Fq "$owned_hash"$'\t'"$owned_relative" "$manifest"
done
if command -v desktop-file-validate >/dev/null 2>&1; then
  desktop-file-validate "$desktop"
fi

env -u WEBKIT_DISABLE_DMABUF_RENDERER "$gui_wrapper" --version \
  | grep -q 'gui --version dmabuf=unset'
env -u WEBKIT_DISABLE_DMABUF_RENDERER \
  WCR_WEBKIT_DISABLE_DMABUF_RENDERER=1 "$gui_wrapper" --version \
  | grep -q 'gui --version dmabuf=1'
WEBKIT_DISABLE_DMABUF_RENDERER=external "$gui_wrapper" --version \
  | grep -q 'gui --version dmabuf=external'
"$cli_bin" restore | grep -q 'cli restore'

before_failed_publish="$(
  sha256sum "$gui_wrapper" "$gui_bin" "$cli_bin" "$desktop" "$icon" "$license" "$manifest"
)"
# shellcheck disable=SC2016 # Write runtime expansions into the v2 fake executable.
printf '#!/bin/sh\nprintf "gui-v2 %%s dmabuf=%%s\\n" "$*" "${WEBKIT_DISABLE_DMABUF_RENDERER-unset}"\n' >"$fake_gui"
chmod +x "$fake_gui"
expect_failure 'Injected install publish failure after 3 file(s)' \
  env \
    WCR_INSTALL_SKIP_BUILD=1 \
    WCR_INSTALL_TAURI_BIN="$fake_gui" \
    WCR_INSTALL_CLI_BIN="$fake_cli" \
    WCR_INSTALL_FRONTEND_DIST="$fake_dist" \
    WCR_INSTALL_FAIL_AFTER_PUBLISH=3 \
    "$ROOT/install.sh" --prefix "$prefix"
after_failed_publish="$(
  sha256sum "$gui_wrapper" "$gui_bin" "$cli_bin" "$desktop" "$icon" "$license" "$manifest"
)"
test "$before_failed_publish" = "$after_failed_publish"
test -s "$gui_wrapper"
test -s "$gui_bin"
assert_no_staging_tree "$prefix"

# A normal upgrade accepts a fully matching owned install and publishes v2.
install_fake "$prefix"
"$gui_wrapper" --version | grep -q 'gui-v2 --version dmabuf=unset'
assert_no_staging_tree "$prefix"

build_only_prefix="$tmp/build-only-prefix"
WCR_INSTALL_SKIP_BUILD=1 \
WCR_INSTALL_TAURI_BIN="$fake_gui" \
WCR_INSTALL_CLI_BIN="$fake_cli" \
WCR_INSTALL_FRONTEND_DIST="$fake_dist" \
  "$ROOT/install.sh" --prefix "$build_only_prefix" --build-only
test ! -e "$build_only_prefix"

"$ROOT/install.sh" --prefix "$prefix" --uninstall
test ! -e "$gui_wrapper"
test ! -e "$gui_bin"
test ! -e "$cli_bin"
test ! -e "$desktop"
test ! -e "$icon"
test ! -e "$license"
test ! -e "$manifest"
assert_no_staging_tree "$prefix"
"$ROOT/install.sh" --prefix "$prefix" --uninstall >/dev/null

# Quotes and backticks are supported by the quoted Desktop Entry Exec value,
# and every printed shell command must remain safe to copy and execute.
quoted_prefix="$tmp/prefix with \"quote\" and \`tick\`"
quoted_install_output="$(install_fake "$quoted_prefix" 2>&1)"
quoted_wrapper="$quoted_prefix/bin/wallpaper-console-gui-rust"
quoted_desktop="$quoted_prefix/share/applications/wallpaper-console-gui-rust.desktop"
test -x "$quoted_wrapper"
"$quoted_wrapper" --version | grep -q 'gui-v2 --version dmabuf=unset'
if command -v desktop-file-validate >/dev/null 2>&1; then
  desktop-file-validate "$quoted_desktop"
fi
path_hint_line="$(grep -F 'export PATH=' <<<"$quoted_install_output")"
path_hint="export PATH=${path_hint_line#*export PATH=}"
hint_path="$(
  env PATH=/usr/bin:/bin bash -c "$path_hint; printf '%s' \"\$PATH\""
)"
test "$hint_path" = "$quoted_prefix/bin:/usr/bin:/bin"
grep -Fq "\\\`tick\\\`" <<<"$quoted_install_output"
grep -Fq "\\\"quote\\\"" <<<"$quoted_install_output"
"$ROOT/install.sh" --prefix "$quoted_prefix" --uninstall >/dev/null
test ! -e "$quoted_wrapper"
assert_no_staging_tree "$quoted_prefix"

# Modified installed files are never removed merely because their path is in
# the manifest; unchanged siblings still uninstall normally.
modified_prefix="$tmp/modified-owned-prefix"
install_fake "$modified_prefix" >/dev/null
modified_cli="$modified_prefix/bin/wallpaper-console-rust"
printf '\nuser-local-change\n' >>"$modified_cli"
expect_failure 'Owned install file was modified; refusing to overwrite it' \
  install_fake "$modified_prefix"
assert_no_staging_tree "$modified_prefix"
modified_uninstall_output="$(
  "$ROOT/install.sh" --prefix "$modified_prefix" --uninstall 2>&1
)"
grep -Fq 'Preserving modified owned file' <<<"$modified_uninstall_output"
grep -Fq 'user-local-change' "$modified_cli"
test ! -e "$modified_prefix/bin/wallpaper-console-gui-rust"
test ! -e "$modified_prefix/share/wallpaper-console-rust/install-manifest-v1"
assert_no_staging_tree "$modified_prefix"
"$ROOT/install.sh" --prefix "$modified_prefix" --uninstall >/dev/null 2>&1
grep -Fq 'user-local-change' "$modified_cli"

# A damaged manifest cannot smuggle an arbitrary deletion path.
malicious_prefix="$tmp/malicious-manifest-prefix"
victim="$tmp/victim-must-survive"
printf 'keep me\n' >"$victim"
mkdir -p "$malicious_prefix/share/wallpaper-console-rust"
{
  printf 'wallpaper-console-rust-install-manifest-v1\n'
  printf '%064d\t../../victim-must-survive\n' 0
} >"$malicious_prefix/share/wallpaper-console-rust/install-manifest-v1"
expect_failure 'Unsafe path in install manifest' \
  "$ROOT/install.sh" --prefix "$malicious_prefix" --uninstall
grep -Fq 'keep me' "$victim"

symlink_prefix="$tmp/symlink-parent-prefix"
outside_bin="$tmp/outside-bin"
mkdir -p "$symlink_prefix" "$outside_bin"
printf 'outside\n' >"$outside_bin/wallpaper-console-rust"
ln -s "$outside_bin" "$symlink_prefix/bin"
expect_failure 'Refusing to follow symbolic-link install directory' \
  "$ROOT/install.sh" --prefix "$symlink_prefix" --uninstall
grep -Fq 'outside' "$outside_bin/wallpaper-console-rust"

ancestor_target="$tmp/ancestor-target"
ancestor_link="$tmp/ancestor-link"
mkdir -p "$ancestor_target"
printf 'ancestor outside\n' >"$ancestor_target/victim"
ln -s "$ancestor_target" "$ancestor_link"
expect_failure 'Install prefix and its ancestors must not be symbolic links' \
  install_fake "$ancestor_link/install-prefix"
grep -Fq 'ancestor outside' "$ancestor_target/victim"
test ! -e "$ancestor_target/install-prefix"

# Replace an owned parent after the initial safety check but before publish.
# A sha256sum shim makes the replacement deterministic while staged payloads
# are hashed; the publish-time recheck must fail before its first rename.
swap_prefix="$tmp/publish-parent-swap-prefix"
swap_outside="$tmp/publish-parent-swap-outside"
swap_tools="$tmp/publish-parent-swap-tools"
swap_marker="$tmp/publish-parent-swap-triggered"
real_sha256sum="$(command -v sha256sum)"
mkdir -p "$swap_prefix" "$swap_outside" "$swap_tools"
printf 'outside publish victim\n' >"$swap_outside/wallpaper-console-gui-rust"
cat >"$swap_tools/sha256sum" <<'EOF_SHA256_SHIM'
#!/usr/bin/env sh
case "$*" in
  *".wallpaper-console-rust.install."*"/bin/wallpaper-console-gui-rust")
    if [ ! -e "$WCR_TEST_SWAP_MARKER" ]; then
      : >"$WCR_TEST_SWAP_MARKER"
      ln -s -- "$WCR_TEST_SWAP_OUTSIDE" "$WCR_TEST_SWAP_PREFIX/bin"
    fi
    ;;
esac
exec "$WCR_TEST_REAL_SHA256SUM" "$@"
EOF_SHA256_SHIM
chmod +x "$swap_tools/sha256sum"
expect_failure 'Refusing to follow symbolic-link install directory' \
  env \
    PATH="$swap_tools:$PATH" \
    WCR_INSTALL_SKIP_BUILD=1 \
    WCR_INSTALL_TAURI_BIN="$fake_gui" \
    WCR_INSTALL_CLI_BIN="$fake_cli" \
    WCR_INSTALL_FRONTEND_DIST="$fake_dist" \
    WCR_TEST_REAL_SHA256SUM="$real_sha256sum" \
    WCR_TEST_SWAP_PREFIX="$swap_prefix" \
    WCR_TEST_SWAP_OUTSIDE="$swap_outside" \
    WCR_TEST_SWAP_MARKER="$swap_marker" \
    "$ROOT/install.sh" --prefix "$swap_prefix"
test -e "$swap_marker"
grep -Fq 'outside publish victim' "$swap_outside/wallpaper-console-gui-rust"
test ! -e "$swap_outside/wallpaper-console-rust"
test ! -e "$swap_prefix/share/wallpaper-console-rust/install-manifest-v1"
assert_no_staging_tree "$swap_prefix"

echo "PASS: install contract"
