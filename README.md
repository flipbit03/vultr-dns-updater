# Vultr DNS Updater

A dynamic DNS (DDNS) client for [Vultr DNS](https://www.vultr.com/docs/introduction-to-vultr-dns/). Automatically detects your public IP address and updates DNS A records on your Vultr-managed domains.

Perfect for keeping a subdomain like `home.yourdomain.com` pointed at your home network's dynamic IP address.

## Requirements

- Python 3.12+
- A domain managed by [Vultr DNS](https://www.vultr.com/docs/introduction-to-vultr-dns/)
- A Vultr API key with DNS permissions

## Installation

```bash
git clone https://github.com/flipbit03/vultr-dns-updater.git
cd vultr-dns-updater
uv sync
uv run vultr-dns --help
```

To install globally to your `$HOME`, use `uv tool install .`

## Quick Start

### 1. Get your Vultr API Key

1. Log in to your [Vultr account](https://my.vultr.com/)
2. Go to **Account** → **API** → **Personal Access Tokens**
3. Create a new API key with DNS permissions

### 2. Create a Configuration File

```bash
vultr-dns init-config
# Creates: ~/.config/vultr-dns-updater/config.toml
```

### 3. Edit the Configuration

```toml
# ~/.config/vultr-dns-updater/config.toml

api_key = "YOUR_VULTR_API_KEY"

[[targets]]
domain = "yourdomain.com"
subdomain = "home"
ttl = 60
```

### 4. Update Your DNS

```bash
vultr-dns status          # Check current status
vultr-dns update          # Update DNS records
vultr-dns update --force  # Force update even if IP hasn't changed
```

## Install as a Systemd Service (requires sudo)

```bash
vultr-dns service install                # Install with 30-minute interval
vultr-dns service install --interval 15  # Custom interval (minutes)
vultr-dns service status                 # Check service status
vultr-dns service uninstall              # Remove the service
```

View logs with `journalctl -u vultr-dns-updater.service -f`

See [COMMANDS.md](COMMANDS.md) for additional commands.

## Configuration

### Without a Config File

You can skip the config file entirely by using flags and environment variables:

```bash
export VULTR_API_KEY="your-api-key"
vultr-dns status --domain yourdomain.com --subdomain home
vultr-dns update --domain yourdomain.com --subdomain home
```

### Config File Locations

| Priority | Location |
|----------|----------|
| 1 | `-c` / `--config` flag |
| 2 | `~/.config/vultr-dns-updater/config.toml` |
| 3 | `~/.vultr-dns-updater.toml` |
| 4 | `./vultr-dns-updater.toml` |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT
