"""Systemd service management commands."""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

import click

from vultr_dns_updater.cli.utils import console, print_error, print_info, print_success
from vultr_dns_updater.config import Config, ConfigError

SERVICE_NAME = "vultr-dns-updater"
SYSTEMD_DIR = Path("/etc/systemd/system")


def _get_vultr_dns_executable() -> str:
    """Get the absolute path to the vultr-dns executable."""
    vultr_dns_path = shutil.which("vultr-dns")
    if vultr_dns_path:
        return str(Path(vultr_dns_path).resolve())

    venv_script = Path(sys.executable).parent / "vultr-dns"
    if venv_script.exists():
        return str(venv_script.resolve())

    return f"{Path(sys.executable).resolve()} -m vultr_dns_updater.cli"


def _run_sudo(args: list[str], capture: bool = False) -> subprocess.CompletedProcess[str]:
    """Run a command with sudo."""
    cmd = ["sudo", *args]
    return subprocess.run(cmd, capture_output=capture, text=True, check=False)


def _run_systemctl(
    args: list[str], use_sudo: bool = False, capture: bool = False
) -> subprocess.CompletedProcess[str]:
    """Run systemctl command, optionally with sudo."""
    if use_sudo:
        return _run_sudo(["systemctl", *args], capture=capture)
    cmd = ["systemctl", *args]
    return subprocess.run(cmd, capture_output=capture, text=True, check=False)


def _generate_service_file(username: str, config_path: Path, executable: str) -> str:
    """Generate systemd service file content."""
    return f"""\
[Unit]
Description=Vultr DNS Updater
After=network-online.target
Wants=network-online.target

[Service]
Type=oneshot
User={username}
ExecStart={executable} update --config {config_path}

[Install]
WantedBy=multi-user.target
"""


def _generate_timer_file(interval: int) -> str:
    """Generate systemd timer file content."""
    return f"""\
[Unit]
Description=Run Vultr DNS Updater every {interval} minutes

[Timer]
OnBootSec=1min
OnUnitActiveSec={interval}min
Persistent=true

[Install]
WantedBy=timers.target
"""


@click.group()
def service() -> None:
    """Manage systemd service for automatic DNS updates."""


@service.command()
@click.option(
    "--interval",
    "-i",
    type=int,
    default=30,
    help="Update interval in minutes (default: 30)",
)
def install(interval: int) -> None:
    """Install systemd service and timer for automatic DNS updates.

    Installs a system-level service that runs as your user.
    Will prompt for sudo password if needed.
    """
    username = os.environ.get("USER", "")
    if not username or username == "root":
        print_error("Cannot determine current user. Don't run as root.")
        raise SystemExit(1)

    try:
        Config.find_and_load()
        config_path = None
        for path in [
            Path.home() / ".config" / "vultr-dns-updater" / "config.toml",
            Path.home() / ".vultr-dns-updater.toml",
            Path("vultr-dns-updater.toml"),
        ]:
            if path.exists():
                config_path = path.resolve()
                break
        if not config_path:
            raise ConfigError("Config file not found")
        config = Config.find_and_load()
        if not config.targets:
            print_error("No targets configured. Add targets to your config file first.")
            raise SystemExit(1)
    except ConfigError as e:
        print_error(f"Configuration required before installing service:\n{e}")
        raise SystemExit(1) from e

    executable = _get_vultr_dns_executable()
    service_file = SYSTEMD_DIR / f"{SERVICE_NAME}.service"
    timer_file = SYSTEMD_DIR / f"{SERVICE_NAME}.timer"

    console.print(f"\n[bold]Installing {SERVICE_NAME} service...[/bold]\n")
    console.print(f"  User: [cyan]{username}[/cyan]")
    console.print(f"  Config: [cyan]{config_path}[/cyan]")
    console.print(f"  Executable: [cyan]{executable}[/cyan]")
    console.print(f"  Interval: [cyan]{interval} minutes[/cyan]\n")

    service_content = _generate_service_file(username, config_path, executable)
    timer_content = _generate_timer_file(interval)

    with tempfile.NamedTemporaryFile(mode="w", suffix=".service", delete=False) as f:
        f.write(service_content)
        temp_service = f.name

    with tempfile.NamedTemporaryFile(mode="w", suffix=".timer", delete=False) as f:
        f.write(timer_content)
        temp_timer = f.name

    console.print("[dim]sudo password may be required...[/dim]\n")

    result = _run_sudo(["cp", temp_service, str(service_file)])
    Path(temp_service).unlink()
    if result.returncode != 0:
        Path(temp_timer).unlink()
        print_error("Failed to install service file. Is sudo available?")
        raise SystemExit(1)
    console.print(f"  Created: {service_file}")

    result = _run_sudo(["cp", temp_timer, str(timer_file)])
    Path(temp_timer).unlink()
    if result.returncode != 0:
        print_error("Failed to install timer file.")
        raise SystemExit(1)
    console.print(f"  Created: {timer_file}")

    console.print("\n[bold]Enabling service...[/bold]\n")

    _run_systemctl(["daemon-reload"], use_sudo=True)
    console.print("  Reloaded systemd daemon")

    result = _run_systemctl(["enable", f"{SERVICE_NAME}.timer"], use_sudo=True)
    if result.returncode != 0:
        print_error("Failed to enable timer")
        raise SystemExit(1)
    console.print(f"  Enabled {SERVICE_NAME}.timer")

    result = _run_systemctl(["start", f"{SERVICE_NAME}.timer"], use_sudo=True)
    if result.returncode != 0:
        print_error("Failed to start timer")
        raise SystemExit(1)
    console.print(f"  Started {SERVICE_NAME}.timer")

    print_success(f"\nService installed! DNS will update every {interval} minutes.")
    console.print(
        f"\n[dim]The service runs as user '{username}' but is managed at system level.[/dim]"
    )
    console.print(f"[dim]View logs:[/dim]  journalctl -u {SERVICE_NAME}.service")
    console.print(f"[dim]Check timer:[/dim] systemctl list-timers {SERVICE_NAME}.timer")


@service.command()
def uninstall() -> None:
    """Uninstall systemd service and timer."""
    service_file = SYSTEMD_DIR / f"{SERVICE_NAME}.service"
    timer_file = SYSTEMD_DIR / f"{SERVICE_NAME}.timer"

    if not service_file.exists() and not timer_file.exists():
        print_info("No service installed. Nothing to uninstall.")
        return

    console.print(f"\n[bold]Uninstalling {SERVICE_NAME} service...[/bold]\n")
    console.print("[dim]sudo password may be required...[/dim]\n")

    _run_systemctl(["stop", f"{SERVICE_NAME}.timer"], use_sudo=True)
    console.print(f"  Stopped {SERVICE_NAME}.timer")

    _run_systemctl(["disable", f"{SERVICE_NAME}.timer"], use_sudo=True)
    console.print(f"  Disabled {SERVICE_NAME}.timer")

    if timer_file.exists():
        result = _run_sudo(["rm", str(timer_file)])
        if result.returncode != 0:
            print_error(f"Failed to remove {timer_file}")
            raise SystemExit(1)
        console.print(f"  Removed: {timer_file}")

    if service_file.exists():
        result = _run_sudo(["rm", str(service_file)])
        if result.returncode != 0:
            print_error(f"Failed to remove {service_file}")
            raise SystemExit(1)
        console.print(f"  Removed: {service_file}")

    _run_systemctl(["daemon-reload"], use_sudo=True)
    console.print("  Reloaded systemd daemon")

    print_success("\nService uninstalled successfully.")


@service.command(name="status")
def service_status() -> None:
    """Check the status of the systemd service and timer."""
    console.print(f"\n[bold]{SERVICE_NAME} Timer Status:[/bold]\n")

    result = _run_systemctl(["status", f"{SERVICE_NAME}.timer"], capture=True)
    if result.returncode == 4:
        print_info("No service installed.\nInstall with: vultr-dns service install")
        return

    console.print(result.stdout or result.stderr)

    console.print("\n[bold]Scheduled Timers:[/bold]\n")
    result = _run_systemctl(["list-timers", f"{SERVICE_NAME}.timer"], capture=True)
    console.print(result.stdout or result.stderr)
