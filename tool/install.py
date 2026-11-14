#!/usr/bin/env python3
"""Install the Core release binaries without changing feature configuration."""
import os
from pathlib import Path
import shutil
import shlex
import subprocess
import tempfile

APP_ID = "remya_core"
PACKAGE = "top.remya.Core1"
UNIT = "top.remya.core.service"
BINARIES = ("remyad", "remya-core-update", "remya")


# Packaged first, then the source install (see crates/updates/src/updater.rs).
UPDATERS = ("/usr/lib/remya-core/remya-core-update", "/usr/local/libexec/remya-core-update")


def data_home(environment):
    value = environment.get("XDG_DATA_HOME", "")
    if value.startswith("/"):
        return Path(value)
    home = environment.get("HOME", "")
    if not home.startswith("/"):
        raise ValueError("An absolute HOME or XDG_DATA_HOME is required")
    return Path(home) / ".local/share"


def unit_quote(path):
    # systemd.syntax: double-quoted word; % and $ are specifier/variable prefixes.
    value = str(path).replace("\\", "\\\\").replace('"', '\\"')
    return '"' + value.replace("%", "%%").replace("$", "$$") + '"'


def user_unit(executable):
    return (
        "[Unit]\nDescription=Remya Core service\n"
        "PartOf=graphical-session.target\nAfter=graphical-session.target\n\n"
        f"[Service]\nType=dbus\nBusName=top.remya.Core1\n"
        f"ExecStart={unit_quote(executable)}\nRestart=on-failure\n\n"
        "[Install]\nWantedBy=graphical-session.target\n")


def stop_service():
    """Ask a running service to quit; it refuses while a system update is running."""
    if not shutil.which("gdbus"):
        return False
    owner = subprocess.run(
        ["gdbus", "call", "--session", "--dest", "org.freedesktop.DBus", "--object-path",
         "/org/freedesktop/DBus", "--method", "org.freedesktop.DBus.NameHasOwner",
         "top.remya.Core1"], capture_output=True, text=True, timeout=10)
    if "true" not in owner.stdout:
        return False
    result = subprocess.run(
        ["gdbus", "call", "--session", "--dest", "top.remya.Core1", "--object-path",
         "/top/remya/Core1", "--method", "top.remya.Core1.Quit"],
        capture_output=True, text=True, timeout=30)
    if result.returncode != 0:
        raise RuntimeError(f"The Core service refused to stop: {result.stderr.strip()}")
    return True


def bundle_running(destination):
    for process in Path("/proc").iterdir():
        if not process.name.isdecimal():
            continue
        try:
            executable = (process / "exe").resolve()
        except (OSError, RuntimeError):
            continue
        if executable.is_relative_to(destination):
            return True
    return False


def install(root, environment):
    data = data_home(environment).resolve()
    source = root / "target/release"
    if not all((source / binary).is_file() for binary in BINARIES):
        raise RuntimeError("Build the Core release binaries first (see docs/xdg-layout.md)")
    destination = data / "remya" / APP_ID
    # Quit refuses while a system update runs. Clients may re-activate the old
    # daemon at once; that is harmless because the bundle is swapped by rename
    # and the unit restart below replaces whichever daemon is running.
    if bundle_running(destination):
        stop_service()
    home = environment.get("HOME", "")
    if not home.startswith("/"):
        raise ValueError("An absolute HOME is required for ~/.local/bin")
    command = Path(home) / ".local/bin/remya"
    if command.exists() or command.is_symlink():
        if not command.is_symlink() or command.resolve() != destination / "remya":
            raise RuntimeError(f"Refusing to replace unrelated command: {command}")
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=f".{APP_ID}-", dir=destination.parent) as temporary:
        staging = Path(temporary) / "bundle"
        staging.mkdir()
        for binary in BINARIES:
            shutil.copy2(source / binary, staging / binary)
        backup = Path(temporary) / "previous"
        if destination.exists():
            destination.rename(backup)
        try:
            staging.rename(destination)
        except OSError:
            if backup.exists():
                backup.rename(destination)
            raise
    services = data / "dbus-1/services"
    services.mkdir(parents=True, exist_ok=True)
    activation = (root / "packaging/top.remya.Core1.service").read_text()
    (services / "top.remya.Core1.service").write_text(
        activation.replace("@REMYAD@", shlex.quote(str(destination / "remyad"))))
    # The resident service starts with the graphical session and owns the daily system update.
    units = data / "systemd/user"
    units.mkdir(parents=True, exist_ok=True)
    (units / UNIT).write_text(user_unit(destination / "remyad"))
    if shutil.which("systemctl"):
        subprocess.run(["systemctl", "--user", "daemon-reload"], check=True)
        subprocess.run(["systemctl", "--user", "enable", UNIT], check=True)
        subprocess.run(["systemctl", "--user", "restart", UNIT], check=True)
    command.parent.mkdir(parents=True, exist_ok=True)
    if not command.is_symlink():
        command.symlink_to(destination / "remya")
    print(f"Installed {PACKAGE} in {destination}")
    print(f"Command: {command} (add ~/.local/bin to PATH if needed)")
    print(f"Service: {UNIT} starts with the graphical session")
    if not any(Path(path).is_file() for path in UPDATERS):
        print("System updates: run sudo python tool/install_system_updater.py once to install the root updater")


if __name__ == "__main__":
    install(Path(__file__).resolve().parents[1], os.environ)
