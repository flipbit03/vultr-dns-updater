"""Pydantic models for Vultr DNS API responses and configuration."""

from __future__ import annotations

from pydantic import BaseModel, Field


class DNSRecord(BaseModel):
    """A DNS record from Vultr API."""

    id: str
    type: str
    name: str
    data: str
    priority: int
    ttl: int


class DNSRecordsResponse(BaseModel):
    """Response from Vultr DNS records list endpoint."""

    records: list[DNSRecord]


class DNSDomain(BaseModel):
    """A DNS domain from Vultr API."""

    domain: str
    date_created: str


class DNSDomainsResponse(BaseModel):
    """Response from Vultr DNS domains list endpoint."""

    domains: list[DNSDomain]


class DNSRecordResponse(BaseModel):
    """Response from create/update DNS record endpoint."""

    record: DNSRecord


class UpdateConfig(BaseModel):
    """Configuration for a single DNS update target."""

    domain: str = Field(description="Base domain (e.g., flipbit03.com)")
    subdomain: str = Field(description="Subdomain to update (e.g., home)")
    ttl: int = Field(default=60, description="TTL in seconds")

    @property
    def fqdn(self) -> str:
        """Get the fully qualified domain name."""
        if self.subdomain:
            return f"{self.subdomain}.{self.domain}"
        return self.domain


class AppConfig(BaseModel):
    """Main application configuration."""

    api_key: str = Field(description="Vultr API key")
    targets: list[UpdateConfig] = Field(default_factory=list, description="DNS update targets")
    ip_check_urls: list[str] = Field(
        default_factory=lambda: [
            "https://api.ipify.org",
            "https://ifconfig.me/ip",
            "https://icanhazip.com",
        ],
        description="URLs to check public IP",
    )
