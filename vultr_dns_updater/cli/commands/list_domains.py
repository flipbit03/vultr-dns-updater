"""List domains command."""

from __future__ import annotations

from pathlib import Path

import click
from rich.table import Table

from vultr_dns_updater.cli.utils import console, print_error, print_info
from vultr_dns_updater.config import Config, ConfigError
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
def list_domains(config_path: Path | None, api_key: str | None) -> None:
    """List all DNS domains in your Vultr account."""
    resolved_api_key = _resolve_api_key(config_path, api_key)
    if not resolved_api_key:
        print_error("No API key provided. Use --api-key or set VULTR_API_KEY")
        raise SystemExit(1)

    try:
        with VultrClient(resolved_api_key) as client:
            domains = client.list_domains()

        if not domains:
            print_info("No domains found in your Vultr account.")
            return

        table = Table(title="Vultr DNS Domains")
        table.add_column("Domain", style="cyan")
        table.add_column("Created", style="dim")

        for domain in domains:
            table.add_row(domain.domain, domain.date_created)

        console.print(table)

    except VultrAPIError as e:
        print_error(str(e))
        raise SystemExit(1) from e
