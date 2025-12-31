# Additional Commands

## `vultr-dns get-ip`

Display your current public IP address.

```bash
$ vultr-dns get-ip
203.0.113.42
```

## `vultr-dns list-domains`

List all DNS domains in your Vultr account.

```bash
$ vultr-dns list-domains
           Vultr DNS Domains
┏━━━━━━━━━━━━━━━━━━┳━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
┃ Domain           ┃ Created                   ┃
┡━━━━━━━━━━━━━━━━━━╇━━━━━━━━━━━━━━━━━━━━━━━━━━━┩
│ yourdomain.com   │ 2024-01-15T10:30:00+00:00 │
│ example.org      │ 2024-02-20T14:45:00+00:00 │
└──────────────────┴───────────────────────────┘
```

## `vultr-dns list-records <domain>`

List all DNS records for a specific domain.

```bash
$ vultr-dns list-records yourdomain.com
                    DNS Records for yourdomain.com
┏━━━━━━━━━━━━━━┳━━━━━━━┳━━━━━━━━┳━━━━━━━━━━━━━━━━━┳━━━━━┳━━━━━━━━━━┓
┃ ID           ┃ Type  ┃ Name   ┃ Data            ┃ TTL ┃ Priority ┃
┡━━━━━━━━━━━━━━╇━━━━━━━╇━━━━━━━━╇━━━━━━━━━━━━━━━━━╇━━━━━╇━━━━━━━━━━┩
│ abc123...    │ A     │ @      │ 192.0.2.1       │ 300 │ 0        │
│ def456...    │ A     │ home   │ 203.0.113.42    │ 60  │ 0        │
│ ghi789...    │ CNAME │ www    │ yourdomain.com  │ 300 │ 0        │
└──────────────┴───────┴────────┴─────────────────┴─────┴──────────┘
```

## `vultr-dns init-config`

Create an example configuration file.

```bash
# Create at default location
vultr-dns init-config

# Create at custom location
vultr-dns init-config --path /path/to/config.toml
```

## `vultr-dns show-config-example`

Print an example configuration to stdout.

```bash
vultr-dns show-config-example
```

## Full Configuration Example

```toml
# Vultr DNS Updater Configuration

api_key = "YOUR_VULTR_API_KEY"

[[targets]]
domain = "example.com"
subdomain = "home"
ttl = 60

[[targets]]
domain = "example.com"
subdomain = "vpn"
ttl = 60

[[targets]]
domain = "another-domain.org"
subdomain = "dynamic"
ttl = 120

# Optional: Custom IP detection URLs
# ip_check_urls = [
#     "https://api.ipify.org",
#     "https://ifconfig.me/ip",
#     "https://icanhazip.com",
# ]
```
