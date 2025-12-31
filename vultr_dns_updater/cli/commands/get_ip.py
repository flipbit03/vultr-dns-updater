"""Get IP command."""

from __future__ import annotations

import click

from vultr_dns_updater.cli.utils import console, print_error
from vultr_dns_updater.ip_service import get_public_ip


@click.command()
def get_ip() -> None:
    """Get the current public IP address."""
    try:
        ip = get_public_ip()
        console.print(ip)
    except RuntimeError as e:
        print_error(str(e))
        raise SystemExit(1) from e
