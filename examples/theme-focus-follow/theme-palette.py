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


def clavis_compatibility():
    setting = os.environ.get('WCR_THEME_COMPAT', 'auto')
    if setting not in ('auto', 'clavis', 'none'):
        raise ValueError('WCR_THEME_COMPAT must be auto, clavis, or none')
    return setting == 'clavis' or (setting == 'auto' and any(path.exists() for path in (
        DATA / 'quickshell/clavis', CACHE_HOME / 'quickshell/personalization.json',
        STATE / 'quickshell/matugen.lock', HOME_DIR / '.local/bin/wcr-post-apply-waybar.sh',
    )))


def read_json(path, default=None):
    try:
        return json.loads(Path(path).read_text())
    except FileNotFoundError:
        return default


def atomic_bytes(path, data):
    path = Path(path).resolve()  # retain user symlinks instead of replacing them
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(prefix='.' + path.name + '.', dir=path.parent)
    try:
        with os.fdopen(fd, 'wb') as stream:
            stream.write(data)
        os.chmod(tmp, path.stat().st_mode & 0o777 if path.exists() else 0o644)
        os.replace(tmp, path)
    finally:
        if os.path.exists(tmp):
            os.unlink(tmp)


def atomic_json(path, data):
    atomic_bytes(path, json.dumps(data, ensure_ascii=False, sort_keys=True).encode())


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


def palette_for(output):
    revision = (read_json(CACHE / 'outputs.json', {}) or {}).get('outputs', {}).get(output)
    if not revision or len(revision) != 64 or any(c not in '0123456789abcdef' for c in revision):
        raise ValueError(f'no complete palette for {output}')
    return read_palette(revision)


def read_palette(revision):
    """Validate the same complete payload before reuse or live activation."""
    directory = CACHE / 'revisions' / revision
    palette = read_json(directory / 'palette.json')
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


def reload_consumers(changed):
    if os.environ.get('WCR_THEME_NO_RELOAD') == '1':
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
    for command in commands:
        try:
            subprocess.run(command, check=True, timeout=1, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        except (OSError, subprocess.SubprocessError) as error:
            pending.extend(changed)
            print(f'theme-palette: reload pending for {command[0]}: {error}', file=sys.stderr)
    return sorted(set(pending))


def activate(output, timeout=30):
    started = time.monotonic()
    with live_locked(timeout):
        revision, payload = palette_for(output)
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
        pending = read_json(pending_path, [])
        remaining = reload_consumers(sorted(set(changed + pending)))
        if remaining != pending:
            atomic_json(pending_path, remaining)
        state = {'output': output, 'revision': revision, 'files': [dict(destination=item['destination'], sha256=item['sha256']) for item, _ in payload]}
        if changed or read_json(CACHE / 'active.json') != state:
            atomic_json(CACHE / 'active.json', state)
    elapsed = (time.monotonic() - started) * 1000
    if changed:
        print(f'theme-palette: activated {output} revision={revision[:12]} files={len(changed)} {elapsed:.1f}ms', flush=True)
    return revision


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
            activate(source)
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


def renderer_active():
    # Linux comm truncates names to 15 bytes, including linux-wallpaperengine.
    return os.environ.get('WCR_FOCUS_REQUIRE_RENDERER', '1') == '0' or any(
        name in ('mpvpaper', 'awww-daemon', 'swaybg', 'linux-wallpaper') for _, name in processes())


def follow_polled(provider):
    current = None; next_check = 0; renderers = False
    while True:
        output = focused_output(provider)
        now = time.monotonic()
        if now >= next_check:
            renderers = renderer_active()
        if output and renderers and (output != current or now >= next_check):
            try:
                activate(output, .02)
                current = output
            except (OSError, ValueError, TimeoutError):
                current = None
        if now >= next_check:
            next_check = now + 1
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
        workspaces = {}; current = None; buffer = b''; due = 0; next_check = 0; renderers = False
        try:
            while stream.poll() is None:
                now = time.monotonic()
                for key, _ in selector.select(.05):
                    chunk = os.read(key.fd, 65536)
                    if not chunk: return
                    buffer += chunk
                    while b'\n' in buffer:
                        line, buffer = buffer.split(b'\n', 1)
                        event = json.loads(line)
                        if 'WorkspacesChanged' in event:
                            workspaces = {w['id']: w for w in event['WorkspacesChanged']['workspaces']}
                            current = next((w['output'] for w in workspaces.values() if w['is_focused']), None)
                            due = now + .04
                        elif 'WorkspaceActivated' in event and event['WorkspaceActivated']['focused']:
                            current = workspaces.get(event['WorkspaceActivated']['id'], {}).get('output')
                            due = now + .04
                if now >= next_check:
                    renderers = renderer_active()
                    next_check = now + 1
                    if not due: due = now
                if current and renderers and due and now >= due:
                    try:
                        # Content comparison also repairs changed palette revisions and external writes
                        # while focus remains on the same output. Busy Clavis gets the next retry.
                        activate(current, .02)
                        due = 0
                    except (OSError, ValueError, TimeoutError):
                        due = now + .25
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
        {'generate': generate, 'activate': lambda: activate(args.output), 'follow': follow,
         'post-apply': post_apply, 'install-service': install_service}[args.command]()
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        print(f'theme-palette: {error}', file=sys.stderr)
        return 1
    return 0

if __name__ == '__main__':
    sys.exit(main())
