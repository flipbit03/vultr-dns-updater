"""Configuration commands."""

from __future__ import annotations

from pathlib import Path

import click

from vultr_dns_updater.cli.utils import console, print_error, print_success
from vultr_dns_updater.config import Config, ConfigError


@click.command()
@click.option(
    "--path",
    "-p",
    type=click.Path(path_type=Path),
    default=None,
    help="Path for the config file (default: ~/.config/vultr-dns-updater/config.toml)",
)
def init_config(path: Path | None) -> None:
    """Create an example configuration file."""
    config_path = path or Config.get_default_config_path()

    try:
        Config.create_example_config(config_path)
        print_success(f"Created example config at: {config_path}")
        console.print("\n[dim]Edit the file to add your Vultr API key and targets.[/dim]")
    except ConfigError as e:
        print_error(str(e))
        raise SystemExit(1) from e


@click.command()
def show_config_example() -> None:
    """Show the example configuration file content."""
    console.print(Config.get_example_config())
