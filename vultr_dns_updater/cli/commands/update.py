"""Update command."""

from __future__ import annotations

from pathlib import Path

import click

from vultr_dns_updater.cli.utils import console, print_error, print_info, print_success
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


def _process_target(
    client: VultrClient,
    target: UpdateConfig,
    ip: str,
    force: bool,
    dry_run: bool,
) -> None:
    """Process a single DNS update target."""
    fqdn = target.fqdn
    console.print(f"\n[bold]Processing:[/bold] {fqdn}")

    existing = client.get_record_by_name(target.domain, target.subdomain, "A")

    if existing:
        console.print(f"  Current: {existing.data} (TTL: {existing.ttl})")

        if existing.data == ip and existing.ttl == target.ttl and not force:
            print_info(f"  {fqdn} is already up to date")
            return

        if dry_run:
            console.print(f"  [yellow]Would update:[/yellow] {existing.data} -> {ip}")
            return

        client.update_record(
            domain=target.domain,
            record_id=existing.id,
            data=ip,
            ttl=target.ttl,
        )
        print_success(f"  Updated {fqdn}: {existing.data} -> {ip}")
    else:
        console.print("  Current: [dim]No record exists[/dim]")

        if dry_run:
            console.print(f"  [yellow]Would create:[/yellow] {target.subdomain} A {ip}")
            return

        client.create_record(
            domain=target.domain,
            name=target.subdomain,
            data=ip,
            record_type="A",
            ttl=target.ttl,
        )
        print_success(f"  Created {fqdn} -> {ip}")


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
    help="Domain to update (overrides config)",
)
@click.option(
    "--subdomain",
    "-s",
    help="Subdomain to update (overrides config)",
)
@click.option(
    "--ttl",
    "-t",
    type=int,
    default=60,
    help="TTL in seconds (default: 60)",
)
@click.option(
    "--force",
    "-f",
    is_flag=True,
    help="Force update even if IP hasn't changed",
)
@click.option(
    "--dry-run",
    is_flag=True,
    help="Show what would be done without making changes",
)
@click.option(
    "--ip",
    "custom_ip",
    help="Use a specific IP instead of detecting automatically",
)
def update(
    config_path: Path | None,
    api_key: str | None,
    domain: str | None,
    subdomain: str | None,
    ttl: int,
    force: bool,
    dry_run: bool,
    custom_ip: str | None,
) -> None:
    """Update DNS records with the current public IP.

    Uses configuration file targets, or specify --domain and --subdomain.
    """
    targets: list[UpdateConfig] = []
    resolved_api_key: str | None = None

    if domain and subdomain:
        targets = [UpdateConfig(domain=domain, subdomain=subdomain, ttl=ttl)]
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
        print_error("No API key provided. Use --api-key, set VULTR_API_KEY, or add to config")
        raise SystemExit(1)

    if not targets:
        print_error(
            "No update targets configured. Add targets to config or use --domain/--subdomain"
        )
        raise SystemExit(1)

    if custom_ip:
        current_ip = custom_ip
        print_info(f"Using custom IP: {current_ip}")
    else:
        try:
            current_ip = get_public_ip()
            print_info(f"Current public IP: {current_ip}")
        except RuntimeError as e:
            print_error(str(e))
            raise SystemExit(1) from e

    if dry_run:
        console.print("\n[bold yellow]DRY RUN MODE - No changes will be made[/bold yellow]\n")

    try:
        with VultrClient(resolved_api_key) as client:
            for target in targets:
                _process_target(
                    client=client,
                    target=target,
                    ip=current_ip,
                    force=force,
                    dry_run=dry_run,
                )
    except VultrAPIError as e:
        print_error(str(e))
        raise SystemExit(1) from e
