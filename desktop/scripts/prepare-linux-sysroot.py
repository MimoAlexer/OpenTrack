#!/usr/bin/env python3
"""Extract pinned Ubuntu headers locally; do not install or change host packages."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

DESKTOP = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--directory', type=Path, default=DESKTOP.parent / 'work/desktop-build')
args = parser.parse_args()
base = args.directory.resolve()
lock = json.loads((DESKTOP / 'linux-packages.lock.json').read_text())
os_release = dict(line.split('=', 1) for line in Path('/etc/os-release').read_text().splitlines() if '=' in line)
architecture = subprocess.check_output(['dpkg', '--print-architecture'], text=True).strip()
if os_release.get('ID', '').strip('"') != lock['distribution'] or os_release.get('VERSION_ID', '').strip('"') != lock['version'] or architecture != lock['architecture']:
    raise SystemExit('This pinned header overlay requires Ubuntu 26.04 amd64. Use native Tauri build dependencies on other systems.')
debs = base / 'debs'
debs.mkdir(parents=True, exist_ok=True)
missing = [p for p in lock['packages'] if not (debs / p['filename']).exists()]
if missing:
    subprocess.run(['apt-get', 'download', *[f"{p['package']}={p['version']}" for p in missing]], cwd=debs, check=True)
for package in lock['packages']:
    archive = debs / package['filename']
    if hashlib.sha256(archive.read_bytes()).hexdigest() != package['sha256']:
        raise SystemExit(f"Package checksum mismatch: {package['package']}; no extraction performed.")
root = base / 'sysroot'
root.mkdir(exist_ok=True)
for package in lock['packages']:
    subprocess.run(['dpkg-deb', '-x', str(debs / package['filename']), str(root)], check=True)
# Relocate overlay metadata only. Installed dependency metadata retains its
# host paths, so already installed development libraries remain usable.
import re
for pc in root.rglob('*.pc'):
    text = pc.read_text()
    text = re.sub(r'(?m)^(prefix|exec_prefix|includedir|libdir|datadir)=/usr(?=/|$)', lambda m: f'{m.group(1)}={root}/usr', text)
    pc.write_text(text)
# Header packages contain unversioned .so links to existing host runtimes.
for path in root.rglob('*'):
    if path.is_symlink() and not path.exists():
        target = path.readlink()
        if target.is_absolute():
            host = target
        else:
            normalized = Path(os.path.normpath(path.parent / target))
            try:
                host = Path('/') / normalized.relative_to(root)
            except ValueError:
                continue
        if host.exists():
            path.unlink()
            path.symlink_to(host.resolve())
print(root)
