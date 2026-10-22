#!/usr/bin/env python3
"""Install the built Linux client and its launcher for the current user."""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

desktop = Path(__file__).resolve().parents[1]
binary = desktop / "target/release/opentrack-desktop"
icon = desktop / "icons/icon.png"
if not sys.platform.startswith("linux") or not binary.is_file():
    raise SystemExit("Build the Linux client with desktop/scripts/build-linux.sh first.")
data = Path(os.environ.get("XDG_DATA_HOME", str(Path.home() / ".local/share")))
if not data.is_absolute() or any(char in str(data) for char in "\r\n\0"):
    raise SystemExit("XDG_DATA_HOME must be an absolute path without control characters.")
destination = data / "opentrack/bin/opentrack-desktop"
icon_destination = data / "icons/hicolor/256x256/apps/app.opentrack.desktop.png"
launcher = data / "applications/app.opentrack.desktop.desktop"
if launcher.exists() and "X-OpenTrack-Managed=true" not in launcher.read_text():
    raise SystemExit("An unmanaged OpenTrack launcher exists; it was not replaced.")
if destination.exists() and not launcher.exists():
    raise SystemExit("An unmanaged OpenTrack executable exists; it was not replaced.")


def atomic_copy(source, target, mode):
    target.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary = tempfile.mkstemp(prefix=".opentrack-", dir=target.parent)
    try:
        with os.fdopen(descriptor, "wb") as output, source.open("rb") as input_file:
            shutil.copyfileobj(input_file, output)
            output.flush()
            os.fsync(output.fileno())
        os.chmod(temporary, mode)
        os.replace(temporary, target)
    finally:
        Path(temporary).unlink(missing_ok=True)


# Desktop Entry Exec quoting has both key-value and command-line escaping.
executable = str(destination).replace("%", "%%")
for character in ("\\", '"', "`", "$"):
    executable = executable.replace(character, "\\" + character)
executable = executable.replace("\\", "\\\\")
icon_value = str(icon_destination).replace("\\", "\\\\")
entry = (
    "[Desktop Entry]\nVersion=1.0\nType=Application\nName=OpenTrack\n"
    "Comment=Your OpenTrack workspace\n"
    f'Exec="{executable}" %u\nIcon={icon_value}\n'
    "Terminal=false\nCategories=Office;\nStartupNotify=true\n"
    "StartupWMClass=opentrack-desktop\n"
    "MimeType=x-scheme-handler/opentrack;\nX-OpenTrack-Managed=true\n"
)
with tempfile.TemporaryDirectory(prefix="opentrack-launcher-") as temporary:
    prepared = Path(temporary) / launcher.name
    prepared.write_text(entry)
    validator = shutil.which("desktop-file-validate")
    if validator:
        subprocess.run([validator, str(prepared)], check=True)
    atomic_copy(binary, destination, 0o755)
    atomic_copy(icon, icon_destination, 0o644)
    atomic_copy(prepared, launcher, 0o644)
updater = shutil.which("update-desktop-database")
if updater:
    subprocess.run([updater, str(launcher.parent)], check=True)
print(f"Installed OpenTrack for this user: {destination}")
print(f"Applications-menu launcher: {launcher}")
print("Open OpenTrack from the applications menu and connect to your server.")
