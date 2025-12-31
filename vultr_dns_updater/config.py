"""Configuration file handling."""

from __future__ import annotations

import tomllib
from pathlib import Path

from pydantic import ValidationError

from vultr_dns_updater.models import AppConfig, UpdateConfig

DEFAULT_CONFIG_PATHS = [
    Path.home() / ".config" / "vultr-dns-updater" / "config.toml",
    Path.home() / ".vultr-dns-updater.toml",
    Path("vultr-dns-updater.toml"),
]

EXAMPLE_CONFIG = """\
# Vultr DNS Updater Configuration
# Place this file at ~/.config/vultr-dns-updater/config.toml

# Your Vultr API key (required)
api_key = "YOUR_VULTR_API_KEY"

# DNS update targets
[[targets]]
domain = "example.com"      # Your domain in Vultr DNS
subdomain = "home"          # Subdomain to update (creates home.example.com)
ttl = 60                    # TTL in seconds (optional, default: 60)

# You can add multiple targets
# [[targets]]
# domain = "example.com"
# subdomain = "work"
# ttl = 60

# Optional: Custom IP check URLs (defaults are usually fine)
# ip_check_urls = [
#     "https://api.ipify.org",
#     "https://ifconfig.me/ip",
#     "https://icanhazip.com",
# ]
"""


class ConfigError(Exception):
    """Configuration error."""


class Config:
    """Configuration manager for the application."""

    def __init__(self, config: AppConfig) -> None:
        """
        Initialize with an AppConfig.

        Args:
            config: The application configuration
        """
        self._config = config

    @property
    def api_key(self) -> str:
        """Get the Vultr API key."""
        return self._config.api_key

    @property
    def targets(self) -> list[UpdateConfig]:
        """Get the DNS update targets."""
        return self._config.targets

    @property
    def ip_check_urls(self) -> list[str]:
        """Get the IP check URLs."""
        return self._config.ip_check_urls

    @classmethod
    def from_file(cls, path: Path) -> Config:
        """
        Load configuration from a TOML file.

        Args:
            path: Path to the configuration file

        Returns:
            Config instance

        Raises:
            ConfigError: If the file cannot be read or parsed
        """
        if not path.exists():
            raise ConfigError(f"Configuration file not found: {path}")

        try:
            with open(path, "rb") as f:
                data = tomllib.load(f)
        except tomllib.TOMLDecodeError as e:
            raise ConfigError(f"Invalid TOML in {path}: {e}") from e

        try:
            config = AppConfig.model_validate(data)
        except ValidationError as e:
            raise ConfigError(f"Invalid configuration in {path}: {e}") from e

        return cls(config)

    @classmethod
    def find_and_load(cls, explicit_path: Path | None = None) -> Config:
        """
        Find and load configuration from default locations.

        Args:
            explicit_path: Explicit path to use (takes precedence)

        Returns:
            Config instance

        Raises:
            ConfigError: If no configuration file is found
        """
        if explicit_path:
            return cls.from_file(explicit_path)

        for path in DEFAULT_CONFIG_PATHS:
            if path.exists():
                return cls.from_file(path)

        searched = "\n".join(f"  - {p}" for p in DEFAULT_CONFIG_PATHS)
        raise ConfigError(
            f"No configuration file found. Searched:\n{searched}\n\n"
            f"Create a config file with:\n  vultr-dns init-config"
        )

    @staticmethod
    def get_default_config_path() -> Path:
        """Get the default configuration file path."""
        return DEFAULT_CONFIG_PATHS[0]

    @staticmethod
    def create_example_config(path: Path) -> None:
        """
        Create an example configuration file.

        Args:
            path: Path where to create the config file

        Raises:
            ConfigError: If the file already exists or cannot be created
        """
        if path.exists():
            raise ConfigError(f"Configuration file already exists: {path}")

        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(EXAMPLE_CONFIG)

    @staticmethod
    def get_example_config() -> str:
        """Get the example configuration content."""
        return EXAMPLE_CONFIG
