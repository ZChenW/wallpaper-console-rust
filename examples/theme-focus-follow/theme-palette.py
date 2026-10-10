#!/usr/bin/env python3
"""Isolated palette generation and serialized, revision-aware activation.

The consumer template configuration is read-only. No matugen command, template
hook, or wallpaper command is run by the focus follower.
"""
import argparse
import contextlib
import fcntl
import hashlib
import json
import os
from pathlib import Path
import selectors
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import tomllib

HERE = Path(__file__).resolve().parent
HOME_DIR = Path.home()
def xdg_home(name, fallback):
    value = Path(os.environ.get(name) or fallback)
    return value if value.is_absolute() else Path(fallback)


CONFIG = xdg_home('XDG_CONFIG_HOME', HOME_DIR / '.config')
CACHE_HOME = xdg_home('XDG_CACHE_HOME', HOME_DIR / '.cache')
CACHE = CACHE_HOME / 'wallpaper-console-rust/theme-palettes'
STATE = xdg_home('XDG_STATE_HOME', HOME_DIR / '.local/state')
DATA = xdg_home('XDG_DATA_HOME', HOME_DIR / '.local/share')


def focus_skipped_templates():
    """Templates too expensive to rewrite on every focus change.

    niri includes its generated colors from the main config, so replacing that
    file makes the compositor reload everything. Those consumers follow the
    wallpaper instead, and are refreshed by the full post-apply activation.
    """
    setting = os.environ.get('WCR_FOCUS_SKIP_TEMPLATES', 'niri')
    return {name.strip() for name in setting.split(',') if name.strip()}


def clavis_reload_command():
    """Ask Clavis to re-read the palette it was just handed.

    This runs inside the palette lock on every focus change, so the launcher
    matters: the `key` wrapper is a Python venv entry point that costs about
    115 ms to start, while QuickShell's own client answers in under 30 ms.
    """
    config = os.environ.get('WCR_CLAVIS_QS_CONFIG', 'clavis')
    qs = shutil.which('qs') or shutil.which('quickshell')
    if qs:
        return [qs, '-c', config, 'ipc', 'call', 'wallpaper', 'reloadColors']
    key = HOME_DIR / '.local/bin/key'
    return [str(key) if key.is_file() else 'key', 'ipc', 'call', 'wallpaper', 'reloadColors']


def clavis_compatibility():
    setting = os.environ.get('WCR_THEME_COMPAT', 'auto')
    if setting not in ('auto', 'clavis', 'none'):
        raise ValueError('WCR_THEME_COMPAT must be auto, clavis, or none')
    return setting == 'clavis' or (setting == 'auto' and any(path.exists() for path in (
        DATA / 'quickshell/clavis', CACHE_HOME / 'quickshell/personalization.json',
        STATE / 'quickshell/matugen.lock', HOME_DIR / '.local/bin/wcr-post-apply-waybar.sh',
    )))


def read_json(path, default=None, _fingerprints=None):
    record_fingerprint(path, _fingerprints)
    try:
        return json.loads(Path(path).read_text())
    except FileNotFoundError:
        return default


def atomic_bytes(path, data, _fingerprints=None):
    consumer_path = Path(path)
    path = consumer_path.resolve()  # retain user symlinks instead of replacing them
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(prefix='.' + path.name + '.', dir=path.parent)
    try:
        with os.fdopen(fd, 'wb') as stream:
            stream.write(data)
        os.chmod(tmp, path.stat().st_mode & 0o777 if path.exists() else 0o644)
        record_fingerprint(consumer_path, _fingerprints, tmp)
        os.replace(tmp, path)
    finally:
        if os.path.exists(tmp):
            os.unlink(tmp)


def atomic_json(path, data, _fingerprints=None):
    atomic_bytes(path, json.dumps(data, ensure_ascii=False, sort_keys=True).encode(), _fingerprints)


@contextlib.contextmanager
def locked(path, timeout=30):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('a') as stream:
        deadline = time.monotonic() + timeout
        while True:
            try:
                fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
                break
            except BlockingIOError:
                if time.monotonic() >= deadline:
                    raise TimeoutError(f'theme lock busy: {path}')
                time.sleep(.02)
        yield


@contextlib.contextmanager
def live_locked(timeout=30):
    # Every WC writer uses the same primary lock, even if Clavis is installed
    # while a follower is running. Only compatibility mode adds its shared lock.
    primary = Path(os.environ.get('WCR_THEME_LOCK') or STATE / 'wallpaper-console-rust/theme.lock').expanduser()
    with locked(primary, timeout):
        legacy = STATE / 'quickshell/matugen.lock'
        if clavis_compatibility() and primary.resolve() != legacy.resolve():
            with locked(legacy, timeout):
                yield
        else:
            yield


def run(args, **kwargs):
    return subprocess.run(args, check=True, capture_output=True, timeout=25, **kwargs)


def template_spec():
    path = Path(os.environ.get('WCR_MATUGEN_CONFIG', CONFIG / 'matugen/config.toml')).expanduser()
    with live_locked():
        config = tomllib.loads(path.read_text())
        personal_path = os.environ.get('WCR_THEME_PERSONALIZATION')
        personal = read_json(Path(personal_path).expanduser(), {}) if personal_path else (
            read_json(CACHE_HOME / 'quickshell/personalization.json', {}) if clavis_compatibility() else {})
        theme = personal.get('theme', {})
        mode = os.environ.get('WCR_THEME_MODE', theme.get('mode', 'dark'))
        scheme = os.environ.get('WCR_THEME_SCHEME', theme.get('matugenScheme', 'scheme-tonal-spot'))
        entries = []
        for name, template in config.get('templates', {}).items():
            src = Path(os.path.expandvars(os.path.expanduser(template['input_path'])))
            if not src.is_absolute():
                src = path.parent / src
            dest = Path(os.path.expandvars(os.path.expanduser(template['output_path'])))
            if not dest.is_absolute():
                dest = path.parent / dest
            entries.append((name, str(dest), src.read_bytes()))
        if os.environ.get('WCR_THEME_GTK', '0') == '1':
            for version in ('3.0', '4.0'):
                dest = str(CONFIG / f'gtk-{version}/wcr-colors.css')
                entries.append((f'gtk-{version}', dest, (HERE.parent / f'matugen/templates/gtk-{version}-gtk.css').read_bytes()))
    if not entries:
        raise ValueError('matugen config has no templates')
    if mode not in ('dark', 'light'):
        raise ValueError(f'unsupported theme mode: {mode}')
    return entries, mode, scheme


def generate():
    manifest_path = Path(os.environ.get('WCR_THEME_MANIFEST', CONFIG / 'wallpaper-console/theme-state.json'))
    with locked(CACHE / 'generate.lock'):
        raw_manifest = manifest_path.read_bytes()
        manifest = json.loads(raw_manifest)
        entries, mode, scheme = template_spec()
        version = run(['matugen', '--version']).stdout
        signature = hashlib.sha256(version + mode.encode() + scheme.encode())
        for name, dest, template in entries:
            signature.update(json.dumps([name, dest]).encode() + template)
        outputs = {}
        for output, entry in manifest.get('outputs', {}).items():
            still = entry.get('still')
            if not still or not Path(still).is_file():
                continue
            digest = signature.copy(); digest.update(Path(still).read_bytes())
            revision = digest.hexdigest()
            dest_dir = CACHE / 'revisions' / revision
            try:
                _, payload = read_palette(revision)
                reusable = [(item['name'], item['destination']) for item, _ in payload] == [
                    (name, dest) for name, dest, _ in entries
                ]
            except (FileNotFoundError, IsADirectoryError, NotADirectoryError, ValueError):
                reusable = False
            if not reusable:
                if dest_dir.exists():
                    # Missing payloads or bad checksums invalidate the whole revision.
                    shutil.rmtree(dest_dir)
                dest_dir.parent.mkdir(parents=True, exist_ok=True)
                with tempfile.TemporaryDirectory(prefix='.render-', dir=dest_dir.parent) as tmp:
                    staging = Path(tmp)
                    sections = ['[config]\nversion_check = false\n']
                    files = []
                    for index, (name, dest, template) in enumerate(entries):
                        source = staging / f'template-{index}'
                        source.write_bytes(template)
                        result = staging / f'file-{index}'
                        # Explicit allowlist: never copy user pre/post hooks or wallpaper settings.
                        sections.append(f'[templates.{json.dumps(name)}]\ninput_path = {json.dumps(str(source))}\noutput_path = {json.dumps(str(result))}\n')
                        files.append({'name': name, 'file': result.name, 'destination': dest})
                    config = staging / 'render.toml'; config.write_text('\n'.join(sections))
                    run(['matugen', '--source-color-index', '0', 'image', still, '--mode', mode, '--type', scheme, '-c', str(config)])
                    for item in files:
                        content = (staging / item['file']).read_bytes()
                        if not content:
                            raise ValueError(f'empty rendered theme: {item["name"]}')
                        item['sha256'] = hashlib.sha256(content).hexdigest()
                    (staging / 'palette.json').write_text(json.dumps({'revision': revision, 'mode': mode, 'files': files}))
                    # Publish the complete immutable directory; incomplete generations stay private.
                    os.rename(staging, dest_dir)
                    staging.mkdir()  # TemporaryDirectory still owns only its disposable pathname
            outputs[output] = revision
        if manifest_path.read_bytes() != raw_manifest:
            raise RuntimeError('wallpaper manifest changed during generation; newer hook must publish it')
        atomic_json(CACHE / 'outputs.json', {'outputs': outputs, 'source': manifest.get('theme_source_output')})
        print(f'theme-palette: cached {len(outputs)} outputs', flush=True)


def palette_for(output, _fingerprints=None):
    revision = (read_json(CACHE / 'outputs.json', {}, _fingerprints) or {}).get('outputs', {}).get(output)
    if not revision or len(revision) != 64 or any(c not in '0123456789abcdef' for c in revision):
        raise ValueError(f'no complete palette for {output}')
    return read_palette(revision, _fingerprints)


def read_palette(revision, _fingerprints=None):
    """Validate the same complete payload before reuse or live activation."""
    directory = CACHE / 'revisions' / revision
    palette = read_json(directory / 'palette.json', _fingerprints=_fingerprints)
    if not isinstance(palette, dict) or palette.get('revision') != revision:
        raise ValueError(f'incomplete palette revision: {revision}')
    files = palette.get('files')
    if not isinstance(files, list) or not files:
        raise ValueError(f'missing palette files: {revision}')
    payload = []
    for index, item in enumerate(files):
        if (not isinstance(item, dict)
                or any(not isinstance(item.get(key), str) or not item[key]
                       for key in ('name', 'file', 'destination', 'sha256'))
                or item['file'] != f'file-{index}'):
            raise ValueError(f'invalid palette file entry: {revision}')
        record_fingerprint(directory / item['file'], _fingerprints)
        content = (directory / item['file']).read_bytes()
        if not content or hashlib.sha256(content).hexdigest() != item['sha256']:
            raise ValueError(f'damaged palette: {item["name"]}')
        payload.append((item, content))
    return revision, payload


def processes():
    for path in Path('/proc').glob('[0-9]*'):
        try:
            if path.stat().st_uid == os.getuid():
                yield path, (path / 'comm').read_text().strip()
        except (OSError, ProcessLookupError):
            pass


# A consumer that cannot be reloaded (Clavis not running, say) stays pending. The follower used to
# activate once a second, so retrying on every activation started a process and logged an error every second
# for as long as the consumer stayed away. Retries back off instead; a fresh change always goes out
# at once, and one-shot commands are new processes, so they always try.
PENDING_RETRY_MAX = 60
pending_retry = {'at': 0.0, 'delay': 1.0}


def pending_retry_due(now):
    return now >= pending_retry['at']


def note_pending_retry(now, failed):
    pending_retry['delay'] = min(PENDING_RETRY_MAX, pending_retry['delay'] * 2) if failed else 1.0
    pending_retry['at'] = now + pending_retry['delay'] if failed else 0.0


def reload_consumers(changed):
    if os.environ.get('WCR_THEME_NO_RELOAD') == '1' or not changed:
        return []
    paths = '\n'.join(changed)
    signals = {}
    if '/kitty/' in paths: signals['kitty'] = signal.SIGUSR1
    if '/btop/' in paths: signals['btop'] = signal.SIGUSR2
    if '/cava/' in paths: signals['cava'] = signal.SIGUSR2
    running = set()
    for proc, name in processes():
        running.add(name)
        if name not in signals:
            continue
        try:
            caught = next(line.split()[1] for line in (proc / 'status').read_text().splitlines() if line.startswith('SigCgt:'))
            sig = signals[name]
            if int(caught, 16) & (1 << (sig - 1)):
                os.kill(int(proc.name), sig)
        except (OSError, StopIteration):
            pass
    commands = []
    pending = []
    if 'fcitx5' in running and '/fcitx5/' in paths:
        commands.append(['fcitx5-remote', '--check', '-r'])
    if 'yazi' in running and '/yazi/' in paths:
        commands.append(['ya', 'emit-to', '0', 'app:theme'])
    # Waybar/niri/QuickShell watch their files. Touch the root Waybar stylesheet
    # rather than restarting both bars on every focus event.
    if '/waybar/' in paths:
        style = CONFIG / 'waybar/style.css'
        if style.exists(): style.touch()
    if any(path.endswith('/clavis/colors.json') or ('/clavis/profiles/' in path and path.endswith('/colors.json'))
           for path in changed):
        commands.append(clavis_reload_command())
    for command in commands:
        try:
            subprocess.run(command, check=True, timeout=1, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        except (OSError, subprocess.SubprocessError) as error:
            pending.extend(changed)
            print(f'theme-palette: reload pending for {command[0]}: {error}', file=sys.stderr)
    return sorted(set(pending))


def activate(output, timeout=30, full=False, _follower=None):
    started = time.monotonic()
    fingerprints = {} if _follower is not None else None
    with live_locked(timeout):
        revision, complete = palette_for(output, fingerprints)
        for item, _ in complete:
            record_fingerprint(item['destination'], fingerprints)
        skipped = set() if full else focus_skipped_templates()
        payload = [entry for entry in complete if entry[0]['name'] not in skipped]
        changed = []
        staged = []
        committed = []
        try:
            # Validate and stage every destination before changing the first one.
            for item, content in payload:
                consumer_path = item['destination']
                dest = Path(consumer_path).resolve()
                original = dest.read_bytes() if dest.exists() else None
                if original == content:
                    continue
                dest.parent.mkdir(parents=True, exist_ok=True)
                fd, temporary = tempfile.mkstemp(prefix='.' + dest.name + '.', dir=dest.parent)
                staged.append((dest, temporary, original, consumer_path))
                with os.fdopen(fd, 'wb') as stream:
                    stream.write(content)
                os.chmod(temporary, dest.stat().st_mode & 0o777 if original is not None else 0o644)
                # The installed inode's stamp must be captured before publishing:
                # an external writer can change the destination immediately after replace.
                record_fingerprint(consumer_path, fingerprints, temporary)
            for dest, temporary, original, consumer_path in staged:
                os.replace(temporary, dest)
                committed.append((dest, original))
                # Reload identifies the consumer by its configured path, even
                # when a symlink stores its contents in a shared directory.
                changed.append(consumer_path)
        except OSError:
            # Keep the previous complete palette when a later replace fails.
            for dest, original in reversed(committed):
                if original is None:
                    dest.unlink(missing_ok=True)
                else:
                    atomic_bytes(dest, original)
            raise
        finally:
            for _, temporary, _, _ in staged:
                Path(temporary).unlink(missing_ok=True)
        pending_path = CACHE / 'pending-reloads.json'
        pending = read_json(pending_path, [], fingerprints)
        now = time.monotonic()
        if changed or (pending and pending_retry_due(now)):
            remaining = reload_consumers(sorted(set(changed + pending)))
            note_pending_retry(now, bool(remaining))
        else:
            remaining = pending
        if remaining != pending:
            atomic_json(pending_path, remaining, fingerprints)
        # Record the complete revision so a partial activation still describes
        # the palette the outputs belong to.
        state = {'output': output, 'revision': revision, 'files': [dict(destination=item['destination'], sha256=item['sha256']) for item, _ in complete]}
        if changed or read_json(CACHE / 'active.json', _fingerprints=fingerprints) != state:
            atomic_json(CACHE / 'active.json', state, fingerprints)
        if _follower is not None:
            _follower.remember(output, revision, complete, remaining, fingerprints)
    elapsed = (time.monotonic() - started) * 1000
    if changed:
        print(f'theme-palette: activated {output} revision={revision[:12]} files={len(changed)} {elapsed:.1f}ms', flush=True)
    return revision


def file_fingerprint(paths):
    stamps = []
    for path in paths:
        try:
            stat = os.stat(path)  # Follow destination symlinks, as activation does.
            stamps.append((stat.st_mtime_ns, stat.st_size, stat.st_ino))
        except (FileNotFoundError, NotADirectoryError):
            stamps.append(None)
    return tuple(stamps)


def record_fingerprint(path, fingerprints, source=None):
    if fingerprints is not None:
        fingerprints[Path(path)] = file_fingerprint((path if source is None else source,))[0]


class FollowerActivation:
    """Only followers may skip an unchanged, previously successful activation."""
    def __init__(self):
        self.output = None
        self.paths = ()
        self.fingerprint = None
        self.last_full = 0
        self.pending = False

    def remember(self, output, revision, complete, pending, fingerprints):
        directory = CACHE / 'revisions' / revision
        paths = [CACHE / 'outputs.json', directory / 'palette.json',
                 CACHE / 'pending-reloads.json', CACHE / 'active.json']
        for item, _ in complete:
            paths.extend((directory / item['file'], Path(item['destination'])))
        self.output = output
        self.paths = paths
        # Stamps describe validated reads or our staged writes, not a later
        # snapshot that could silently absorb an external writer's changes.
        self.fingerprint = tuple(fingerprints[path] for path in paths)
        self.last_full = time.monotonic()
        self.pending = bool(pending)

    def deadline(self):
        if self.output is None:
            return float('inf')
        safety = self.last_full + 30
        return min(safety, pending_retry['at']) if self.pending else safety

    def activate(self, output, now, force=False):
        # The one-second metadata check catches ordinary edits, creates, deletes,
        # atomic replacements and symlink retargets of the manifest, revision,
        # destinations and reload/active state. Same-size writes that preserve
        # mtime and inode need the 30-second full content
        # comparison/checksum validation. No file contents or locks are touched
        # on an unchanged idle tick. Focus events always force the full path.
        if (force or output != self.output or now >= self.deadline()
                or file_fingerprint(self.paths) != self.fingerprint):
            self.output = None  # A failed activation must never leave a usable memo.
            activate(output, .02, _follower=self)


def focus_provider():
    provider = os.environ.get('WCR_FOCUS_PROVIDER', 'auto')
    if provider not in ('auto', 'niri', 'hyprland', 'sway', 'custom'):
        raise ValueError('WCR_FOCUS_PROVIDER must be auto, niri, hyprland, sway, or custom')
    if provider != 'auto':
        return provider
    if os.environ.get('WCR_FOCUS_COMMAND'):
        return 'custom'
    for desktop in os.environ.get('XDG_CURRENT_DESKTOP', '').lower().split(':'):
        if desktop in ('niri', 'hyprland', 'sway'):
            return desktop
    for variable, name in (('NIRI_SOCKET', 'niri'), ('HYPRLAND_INSTANCE_SIGNATURE', 'hyprland'), ('SWAYSOCK', 'sway')):
        if os.environ.get(variable):
            return name
    return None


def focused_output(provider=None):
    provider = provider or focus_provider()
    commands = {'niri': ['niri', 'msg', '-j', 'focused-output'],
                'hyprland': ['hyprctl', '-j', 'monitors'],
                'sway': ['swaymsg', '-r', '-t', 'get_outputs']}
    if provider == 'custom':
        command = json.loads(os.environ.get('WCR_FOCUS_COMMAND', '[]'))
        if not isinstance(command, list) or not command or any(not isinstance(arg, str) or not arg for arg in command):
            raise ValueError('WCR_FOCUS_COMMAND must be a nonempty JSON array of command arguments')
    elif provider in commands:
        command = commands[provider]
    else:
        return None
    try:
        value = json.loads(subprocess.run(command, check=True, capture_output=True, timeout=1).stdout)
        if provider in ('niri', 'custom'):
            name = value.get('name') if isinstance(value, dict) else None
        else:
            name = next((item.get('name') for item in value
                         if isinstance(item, dict) and item.get('focused') is True), None) if isinstance(value, list) else None
        return name if isinstance(name, str) and name else None
    except (OSError, ValueError, subprocess.SubprocessError):
        return None


def post_apply():
    bridge_path = os.environ.get('WCR_ORIGINAL_POST_APPLY')
    bridge = Path(bridge_path).expanduser() if bridge_path else (
        HOME_DIR / '.local/bin/wcr-post-apply-waybar.sh' if clavis_compatibility() else None)
    ready = False
    try:
        generate()
        source = focused_output() or read_json(CACHE / 'outputs.json', {}).get('source')
        if source:
            activate(source, full=True)
            ready = True
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        print(f'theme-palette: cache unavailable, preserving original hook: {error}', file=sys.stderr)
    if bridge is not None and bridge.is_file():
        env = dict(os.environ)
        # Only the explicitly integrated bridge understands this optimization.
        if ready: env['WCR_THEME_PREGENERATED'] = '1'
        subprocess.run([str(bridge)], env=env, check=True, timeout=30)
    elif not ready:
        raise RuntimeError('neither cached palette nor original post-apply hook is available')


renderer_pids = {}


def renderer_active():
    # Linux comm truncates names to 15 bytes, including linux-wallpaperengine.
    if os.environ.get('WCR_FOCUS_REQUIRE_RENDERER', '1') == '0':
        return True
    for path, name in list(renderer_pids.items()):
        try:
            if path.stat().st_uid == os.getuid() and (path / 'comm').read_text().strip() == name:
                return True
        except OSError:
            pass
        del renderer_pids[path]
    renderer_pids.update((path, name) for path, name in processes()
                         if name in ('mpvpaper', 'awww-daemon', 'swaybg', 'linux-wallpaper'))
    return bool(renderer_pids)


def follow_polled(provider):
    current = None; next_check = 0; renderers = False
    due = None; activation = FollowerActivation()
    while True:
        output = focused_output(provider)
        now = time.monotonic()
        if output != current:
            current = output
            due = now if output else None
        tick = now >= next_check
        if tick:
            renderers = renderer_active()
            next_check = time.monotonic() + 1
        if output and renderers and (
                (due is not None and now >= due)
                or (due is None and (tick or now >= activation.deadline()))):
            try:
                activation.activate(output, now, force=due is not None)
                due = None
            except (OSError, ValueError, TimeoutError):
                due = time.monotonic() + .25
        time.sleep(.15)


def follow():
    with locked(CACHE / 'follow.lock', 0):
        provider = focus_provider()
        if provider is None:
            raise RuntimeError('no supported focus provider detected; set WCR_FOCUS_PROVIDER or WCR_FOCUS_COMMAND')
        if provider != 'niri':
            return follow_polled(provider)
        stream = subprocess.Popen(['niri', 'msg', '-j', 'event-stream'], stdout=subprocess.PIPE)
        selector = selectors.DefaultSelector(); selector.register(stream.stdout, selectors.EVENT_READ)
        workspaces = {}; current = None; buffer = b''; due = None; next_check = 0; renderers = False
        activation = FollowerActivation()
        try:
            while True:
                now = time.monotonic()
                deadlines = [next_check]
                if current and renderers:
                    deadlines.append(due if due is not None else activation.deadline())
                # EOF on the pipe notices stream exit; polling the child at 20 Hz
                # is unnecessary. Focus events wake select even during idle waits.
                for key, _ in selector.select(max(0, min(deadlines) - now)):
                    chunk = os.read(key.fd, 65536)
                    if not chunk: return
                    buffer += chunk
                    while b'\n' in buffer:
                        line, buffer = buffer.split(b'\n', 1)
                        event = json.loads(line)
                        if 'WorkspacesChanged' in event:
                            workspaces = {w['id']: w for w in event['WorkspacesChanged']['workspaces']}
                            current = next((w['output'] for w in workspaces.values() if w['is_focused']), None)
                            due = time.monotonic() + .04 if current else None
                        elif 'WorkspaceActivated' in event and event['WorkspaceActivated']['focused']:
                            current = workspaces.get(event['WorkspaceActivated']['id'], {}).get('output')
                            due = time.monotonic() + .04 if current else None
                now = time.monotonic()
                tick = now >= next_check
                if tick:
                    renderers = renderer_active()
                    next_check = time.monotonic() + 1
                if current and renderers and (
                        (due is not None and now >= due)
                        or (due is None and (tick or now >= activation.deadline()))):
                    try:
                        # Content comparison also repairs changed palette revisions and external writes
                        # while focus remains unchanged; metadata decides when it is needed.
                        activation.activate(current, now, force=due is not None)
                        due = None
                    except (OSError, ValueError, TimeoutError):
                        due = time.monotonic() + .25
        finally:
            selector.close()
            stream.terminate()
            try: stream.wait(timeout=2)
            except subprocess.TimeoutExpired: stream.kill(); stream.wait()


def systemd_command_arg(value):
    value = str(value)
    if any(ord(char) < 32 for char in value):
        raise ValueError('service paths cannot contain control characters')
    value = value.replace('\\', '\\\\').replace('"', '\\"').replace('%', '%%')
    return '"' + value.replace('$', '$$') + '"'


def install_service():
    unit = (HERE / 'wcr-theme-focus.service').read_text()
    unit = unit.replace('@PYTHON@', systemd_command_arg(sys.executable))
    unit = unit.replace('@SCRIPT@', systemd_command_arg(HERE / 'theme-palette.py'))
    path = CONFIG / 'systemd/user/wcr-theme-focus.service'
    atomic_bytes(path, unit.encode())
    print(path)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=['generate', 'activate', 'follow', 'post-apply', 'install-service'])
    parser.add_argument('output', nargs='?')
    args = parser.parse_args()
    if args.command == 'activate' and not args.output: parser.error('activate requires an output')
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))
    try:
        {'generate': generate, 'activate': lambda: activate(args.output, full=True), 'follow': follow,
         'post-apply': post_apply, 'install-service': install_service}[args.command]()
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        print(f'theme-palette: {error}', file=sys.stderr)
        return 1
    return 0

if __name__ == '__main__':
    sys.exit(main())
