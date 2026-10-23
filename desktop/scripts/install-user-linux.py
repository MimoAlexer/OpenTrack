#!/usr/bin/env python3
"""Install the built Linux client and its launcher for the current user."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

desktop = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__, exit_on_error=False)
parser.add_argument("--binary", type=Path, metavar="ABSOLUTE_PATH",
                    help="Select a built client instead of desktop/target/release/opentrack-desktop")
try:
    arguments, unknown = parser.parse_known_args()
except argparse.ArgumentError:
    raise SystemExit("Use --binary ABSOLUTE_PATH to select a built Linux client.") from None
if unknown:
    raise SystemExit("Unsupported installer arguments.")
binary = arguments.binary if arguments.binary is not None else desktop / "target/release/opentrack-desktop"
try:
    if arguments.binary is not None:
        if not binary.is_absolute() or any(ord(char) < 32 or 127 <= ord(char) <= 159 for char in str(binary)):
            raise SystemExit("The selected binary must be an absolute path without control characters.")
        if binary.is_symlink() or not binary.is_file():
            raise SystemExit("The selected binary must be an existing regular file, not a symbolic link.")
    if not sys.platform.startswith("linux") or not binary.is_file():
        raise SystemExit("Build the Linux client with desktop/scripts/build-linux.sh first.")
except OSError:
    raise SystemExit("The Linux client source could not be inspected.") from None
icon = desktop / "icons/icon.png"
data = Path(os.environ.get("XDG_DATA_HOME", str(Path.home() / ".local/share")))
if not data.is_absolute() or any(char in str(data) for char in "\r\n\0"):
    raise SystemExit("XDG_DATA_HOME must be an absolute path without control characters.")
destination = data / "opentrack/bin/opentrack-desktop"
icon_destination = data / "icons/hicolor/256x256/apps/app.opentrack.desktop.png"
launcher = data / "applications/app.opentrack.desktop.desktop"
try:
    if launcher.exists() and b"X-OpenTrack-Managed=true" not in launcher.read_bytes():
        raise SystemExit("An unmanaged OpenTrack launcher exists; it was not replaced.")
    if destination.exists() and not launcher.exists():
        raise SystemExit("An unmanaged OpenTrack executable exists; it was not replaced.")
except OSError:
    raise SystemExit("Existing user installation could not be inspected.") from None


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
try:
    with tempfile.TemporaryDirectory(prefix="opentrack-launcher-") as temporary:
        prepared = Path(temporary) / launcher.name
        prepared.write_text(entry)
        validator = shutil.which("desktop-file-validate")
        if validator:
            try:
                subprocess.run([validator, str(prepared)], check=True, capture_output=True, timeout=10)
            except (OSError, subprocess.SubprocessError):
                raise SystemExit("Desktop launcher validation failed; existing files were retained.") from None
        atomic_copy(binary, destination, 0o755)
        atomic_copy(icon, icon_destination, 0o644)
        atomic_copy(prepared, launcher, 0o644)
except OSError:
    raise SystemExit("The user installation could not be completed.") from None
updater = shutil.which("update-desktop-database")
if updater:
    try:
        subprocess.run([updater, str(launcher.parent)], check=True, capture_output=True, timeout=10)
    except (OSError, subprocess.SubprocessError):
        raise SystemExit("Launcher cache update failed; installed files were retained.") from None
print("Installed OpenTrack for the current user.")
print("Applications-menu launcher installed.")
print("Open OpenTrack from the applications menu and connect to your server.")
