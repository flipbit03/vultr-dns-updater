"""Vultr DNS API client."""

from __future__ import annotations

from typing import TYPE_CHECKING

import httpx

from vultr_dns_updater.models import (
    DNSDomain,
    DNSDomainsResponse,
    DNSRecord,
    DNSRecordResponse,
    DNSRecordsResponse,
)

if TYPE_CHECKING:
    pass

VULTR_API_BASE = "https://api.vultr.com/v2"


class VultrAPIError(Exception):
    """Exception for Vultr API errors."""

    def __init__(self, status_code: int, message: str) -> None:
        self.status_code = status_code
        self.message = message
        super().__init__(f"Vultr API Error ({status_code}): {message}")


class VultrClient:
    """Client for interacting with the Vultr DNS API."""

    def __init__(self, api_key: str, timeout: float = 30.0) -> None:
        """
        Initialize the Vultr API client.

        Args:
            api_key: Vultr API key
            timeout: Request timeout in seconds
        """
        self.api_key = api_key
        self.timeout = timeout
        self._client = httpx.Client(
            base_url=VULTR_API_BASE,
            headers={
                "Authorization": f"Bearer {api_key}",
                "Content-Type": "application/json",
            },
            timeout=timeout,
        )

    def __enter__(self) -> VultrClient:
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_val: BaseException | None,
        exc_tb: object,
    ) -> None:
        self.close()

    def close(self) -> None:
        """Close the HTTP client."""
        self._client.close()

    def _handle_response(self, response: httpx.Response) -> dict[str, object]:
        """Handle API response and raise on errors."""
        if response.status_code >= 400:
            try:
                error_data = response.json()
                message = error_data.get("error", response.text)
            except Exception:
                message = response.text
            raise VultrAPIError(response.status_code, str(message))

        if response.status_code == 204:  # No content
            return {}

        return dict(response.json())

    def list_domains(self) -> list[DNSDomain]:
        """
        List all DNS domains in the account.

        Returns:
            List of DNS domains
        """
        response = self._client.get("/domains")
        data = self._handle_response(response)
        parsed = DNSDomainsResponse.model_validate(data)
        return parsed.domains

    def list_records(self, domain: str) -> list[DNSRecord]:
        """
        List all DNS records for a domain.

        Args:
            domain: The domain name

        Returns:
            List of DNS records
        """
        response = self._client.get(f"/domains/{domain}/records")
        data = self._handle_response(response)
        parsed = DNSRecordsResponse.model_validate(data)
        return parsed.records

    def get_record_by_name(
        self,
        domain: str,
        name: str,
        record_type: str = "A",
    ) -> DNSRecord | None:
        """
        Find a specific DNS record by name and type.

        Args:
            domain: The domain name
            name: The record name (subdomain)
            record_type: The record type (default: A)

        Returns:
            The DNS record if found, None otherwise
        """
        records = self.list_records(domain)
        for record in records:
            if record.name == name and record.type == record_type:
                return record
        return None

    def create_record(
        self,
        domain: str,
        name: str,
        data: str,
        record_type: str = "A",
        ttl: int = 300,
        priority: int = 0,
    ) -> DNSRecord:
        """
        Create a new DNS record.

        Args:
            domain: The domain name
            name: The record name (subdomain)
            data: The record data (e.g., IP address)
            record_type: The record type (default: A)
            ttl: Time to live in seconds (default: 300)
            priority: Record priority (default: 0)

        Returns:
            The created DNS record
        """
        payload = {
            "name": name,
            "type": record_type,
            "data": data,
            "ttl": ttl,
            "priority": priority,
        }
        response = self._client.post(f"/domains/{domain}/records", json=payload)
        resp_data = self._handle_response(response)
        parsed = DNSRecordResponse.model_validate(resp_data)
        return parsed.record

    def update_record(
        self,
        domain: str,
        record_id: str,
        name: str | None = None,
        data: str | None = None,
        ttl: int | None = None,
        priority: int | None = None,
    ) -> None:
        """
        Update an existing DNS record.

        Args:
            domain: The domain name
            record_id: The record ID
            name: New record name (optional)
            data: New record data (optional)
            ttl: New TTL (optional)
            priority: New priority (optional)
        """
        payload: dict[str, str | int] = {}
        if name is not None:
            payload["name"] = name
        if data is not None:
            payload["data"] = data
        if ttl is not None:
            payload["ttl"] = ttl
        if priority is not None:
            payload["priority"] = priority

        response = self._client.patch(
            f"/domains/{domain}/records/{record_id}",
            json=payload,
        )
        self._handle_response(response)

    def delete_record(self, domain: str, record_id: str) -> None:
        """
        Delete a DNS record.

        Args:
            domain: The domain name
            record_id: The record ID
        """
        response = self._client.delete(f"/domains/{domain}/records/{record_id}")
        self._handle_response(response)

    def ensure_record(
        self,
        domain: str,
        name: str,
        data: str,
        record_type: str = "A",
        ttl: int = 300,
        force: bool = False,
    ) -> tuple[DNSRecord, bool]:
        """
        Ensure a DNS record exists with the specified data.

        Creates the record if it doesn't exist, updates it if the data changed.

        Args:
            domain: The domain name
            name: The record name (subdomain)
            data: The record data (e.g., IP address)
            record_type: The record type (default: A)
            ttl: Time to live in seconds (default: 300)
            force: Force update even if data matches (default: False)

        Returns:
            Tuple of (record, was_changed)
        """
        existing = self.get_record_by_name(domain, name, record_type)

        if existing is None:
            # Create new record
            record = self.create_record(domain, name, data, record_type, ttl)
            return record, True

        if existing.data == data and existing.ttl == ttl and not force:
            # No change needed
            return existing, False

        # Update existing record
        self.update_record(domain, existing.id, data=data, ttl=ttl)
        # Return updated record info
        updated = DNSRecord(
            id=existing.id,
            type=existing.type,
            name=existing.name,
            data=data,
            priority=existing.priority,
            ttl=ttl,
        )
        return updated, True
