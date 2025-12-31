"""IP address detection service."""

from __future__ import annotations

import httpx

DEFAULT_IP_CHECK_URLS = [
    "https://api.ipify.org",
    "https://ifconfig.me/ip",
    "https://icanhazip.com",
]


def get_public_ip(
    urls: list[str] | None = None,
    timeout: float = 10.0,
) -> str:
    """
    Get the current public IP address.

    Tries multiple services in order until one succeeds.

    Args:
        urls: List of URLs to try for IP detection
        timeout: Request timeout in seconds

    Returns:
        The public IP address as a string

    Raises:
        RuntimeError: If all IP detection services fail
    """
    check_urls = urls or DEFAULT_IP_CHECK_URLS
    errors: list[str] = []

    for url in check_urls:
        try:
            response = httpx.get(url, timeout=timeout, follow_redirects=True)
            response.raise_for_status()
            ip = response.text.strip()
            # Basic validation - should be a valid IP
            if _is_valid_ipv4(ip):
                return ip
            errors.append(f"{url}: Invalid IP format: {ip}")
        except httpx.HTTPError as e:
            errors.append(f"{url}: {e}")

    raise RuntimeError(
        "Failed to detect public IP. Errors:\n" + "\n".join(f"  - {e}" for e in errors)
    )


def _is_valid_ipv4(ip: str) -> bool:
    """Check if a string is a valid IPv4 address."""
    parts = ip.split(".")
    if len(parts) != 4:
        return False
    try:
        return all(0 <= int(part) <= 255 for part in parts)
    except ValueError:
        return False
