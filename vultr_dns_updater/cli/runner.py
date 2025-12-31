"""CLI runner - main entry point that aggregates all commands."""

from __future__ import annotations

import click

from vultr_dns_updater.cli.commands.config import init_config, show_config_example
from vultr_dns_updater.cli.commands.get_ip import get_ip
from vultr_dns_updater.cli.commands.list_domains import list_domains
from vultr_dns_updater.cli.commands.list_records import list_records
from vultr_dns_updater.cli.commands.service import service
from vultr_dns_updater.cli.commands.status import status
from vultr_dns_updater.cli.commands.update import update


@click.group()
@click.version_option(package_name="vultr-dns-updater")
def cli() -> None:
    """Vultr DNS Updater - Dynamic DNS for Vultr DNS service."""


# Register commands
cli.add_command(get_ip)
cli.add_command(list_domains)
cli.add_command(list_records)
cli.add_command(status)
cli.add_command(update)
cli.add_command(init_config)
cli.add_command(show_config_example)
cli.add_command(service)


if __name__ == "__main__":
    cli()
