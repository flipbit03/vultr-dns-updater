"""Status command."""

from __future__ import annotations

from pathlib import Path

import click
from rich.table import Table

from vultr_dns_updater.cli.utils import console, print_error
from vultr_dns_updater.config import Config, ConfigError
from vultr_dns_updater.ip_service import get_public_ip
from vultr_dns_updater.models import UpdateConfig
from vultr_dns_updater.vultr_client import VultrAPIError, VultrClient


def _resolve_api_key(config_path: Path | None, api_key: str | None) -> str | None:
    """Resolve API key from various sources."""
    if api_key:
        return api_key

    try:
        config = Config.find_and_load(config_path)
        return config.api_key
    except ConfigError:
        return None


@click.command()
@click.option(
    "--config",
    "-c",
    "config_path",
    type=click.Path(exists=True, path_type=Path),
    default=None,
    help="Path to configuration file",
)
@click.option(
    "--api-key",
    envvar="VULTR_API_KEY",
    help="Vultr API key (or set VULTR_API_KEY env var)",
)
@click.option(
    "--domain",
    "-d",
    help="Domain to check",
)
@click.option(
    "--subdomain",
    "-s",
    help="Subdomain to check",
)
def status(
    config_path: Path | None,
    api_key: str | None,
    domain: str | None,
    subdomain: str | None,
) -> None:
    """Check the current status of DNS records vs public IP."""
    targets: list[UpdateConfig] = []
    resolved_api_key: str | None = None

    if domain and subdomain:
        targets = [UpdateConfig(domain=domain, subdomain=subdomain)]
        resolved_api_key = _resolve_api_key(config_path, api_key)
    else:
        try:
            config = Config.find_and_load(config_path)
            targets = config.targets
            resolved_api_key = api_key or config.api_key
        except ConfigError as e:
            if domain or subdomain:
                print_error("Both --domain and --subdomain are required together")
            else:
                print_error(str(e))
            raise SystemExit(1) from e

    if not resolved_api_key:
        print_error("No API key provided")
        raise SystemExit(1)

    if not targets:
        print_error("No targets configured")
        raise SystemExit(1)

    try:
        current_ip = get_public_ip()
    except RuntimeError as e:
        print_error(str(e))
        raise SystemExit(1) from e

    console.print(f"[bold]Current Public IP:[/bold] {current_ip}\n")

    try:
        with VultrClient(resolved_api_key) as client:
            table = Table(title="DNS Record Status")
            table.add_column("FQDN", style="cyan")
            table.add_column("Current DNS", style="yellow")
            table.add_column("Status", style="bold")

            for target in targets:
                existing = client.get_record_by_name(target.domain, target.subdomain, "A")

                if existing:
                    if existing.data == current_ip:
                        status_str = "[green]Up to date[/green]"
                    else:
                        status_str = "[red]Needs update[/red]"
                    dns_ip = existing.data
                else:
                    status_str = "[yellow]Not found[/yellow]"
                    dns_ip = "-"

                table.add_row(target.fqdn, dns_ip, status_str)

            console.print(table)

    except VultrAPIError as e:
        print_error(str(e))
        raise SystemExit(1) from e
