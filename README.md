# Vultr DNS Updater

A dynamic DNS (DDNS) client for [Vultr DNS](https://www.vultr.com/docs/introduction-to-vultr-dns/). It detects your public IPv4 address and creates or updates A records on your Vultr-managed domains.

Perfect for keeping a subdomain like `home.yourdomain.com` pointed at your home network's dynamic IP address.

One static binary, `vultr-dns`: no Python, no libc, no system TLS library, so an OS upgrade can't break it.

## Requirements

- A domain managed by [Vultr DNS](https://www.vultr.com/docs/introduction-to-vultr-dns/)
- A Vultr API key with DNS permissions
- Linux (x86_64 or aarch64) or macOS (Apple Silicon) for the prebuilt binaries; anything Rust builds on with `cargo install`

## Installation

```bash
curl -fsSL https://raw.githubusercontent.com/flipbit03/vultr-dns-updater/main/install.sh | sh
```

The installer puts `vultr-dns` in `~/.local/bin` (set `VULTR_DNS_INSTALL_DIR` to change it), verifies the download against the release's `.sha256` before installing, and refuses if it can't. To install a specific release, set `VULTR_DNS_VERSION=v1.2.3`.

Other ways:

```bash
cargo install vultr-dns-updater --locked    # from crates.io
```

or download `vultr-dns_<os>_<arch>` and its `.sha256` from [GitHub Releases](https://github.com/flipbit03/vultr-dns-updater/releases). Every release binary carries a build provenance attestation:

```bash
gh attestation verify vultr-dns_linux_x86_64 -R flipbit03/vultr-dns-updater
```

### Updating

```bash
vultr-dns self update --check   # is there a newer release?
vultr-dns self update           # install it
```

A release binary downloads the new one, checks its SHA-256 and that it runs, keeps the current one as `vultr-dns.old` next to it, and swaps it in atomically. To roll back: `mv vultr-dns.old vultr-dns`. A `cargo install` build runs `cargo install vultr-dns-updater --locked` again. Updates never happen on their own: the timer's `update` never checks for or installs a new version.

## Quick Start

### 1. Get your Vultr API Key

1. Log in to your [Vultr account](https://my.vultr.com/)
2. Go to **Account** → **API** → **Personal Access Tokens**
3. Create a new API key with DNS permissions

### 2. Create a Configuration File

```bash
vultr-dns init-config
# Creates: ~/.config/vultr-dns-updater/config.toml (mode 0600)
```

### 3. Edit the Configuration

```toml
# ~/.config/vultr-dns-updater/config.toml

api_key = "YOUR_VULTR_API_KEY"

[[targets]]
domain = "yourdomain.com"
subdomain = "home"   # "" for the domain itself (the apex)
ttl = 60             # optional, default 60

# Optional: where to ask for the public IP, in order (these are the defaults)
# ip_check_urls = ["https://api.ipify.org", "https://ifconfig.me/ip", "https://icanhazip.com"]
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

This writes `/etc/systemd/system/vultr-dns-updater.{service,timer}`: a system-level service that runs `vultr-dns update --config <your config>` as your user, a minute after boot and then every interval. View logs with `journalctl -u vultr-dns-updater.service -f`.

## Commands

| Command | What it does |
|---|---|
| `update` | Detect the public IP and create or update every target's A record. `--dry-run` shows what would change; `-f/--force` writes even when IP and TTL match; `--ip IPV4` skips detection; `-d DOMAIN -s SUB [-t TTL]` updates one target from flags instead of the config. Every target is tried even if one fails; the exit code is 1 if any failed. |
| `status` | The public IP and, per target, `Up to date`, `Needs update` (IP or TTL differ) or `Not found`. Takes `-d`/`-s` too. |
| `get-ip` | Print the detected public IPv4. |
| `list-domains` | The domains in your Vultr account. |
| `list-records DOMAIN` | The records of one domain (`@` is the apex). |
| `init-config [-p PATH]` | Write the example config (refuses to overwrite). |
| `show-config-example` | Print the example config. |
| `service install\|uninstall\|status` | Manage the systemd timer (see above). |
| `usage` | A compact reference of every command, for AI agents. |
| `self update [--check]` | Update vultr-dns itself (see above). |

`status`, `list-domains` and `list-records` take `--format json`. Commands that talk to Vultr take `-c/--config PATH` and `--api-key KEY`; `get-ip` takes `-c` so a config's `ip_check_urls` apply. `vultr-dns <command> --help` shows every flag.

Exit codes: 0 success (including "already up to date" and dry runs), 1 any runtime error, 2 a usage error (a bad flag, `-d` without `-s`, a `--config` path that doesn't exist).

## Configuration

### Without a Config File

You can skip the config file entirely by using flags and environment variables:

```bash
export VULTR_API_KEY="your-api-key"
vultr-dns status --domain yourdomain.com --subdomain home
vultr-dns update --domain yourdomain.com --subdomain home
```

The API key comes from `--api-key`, then `VULTR_API_KEY`, then `api_key` in the config file.

### Config File Locations

| Priority | Location |
|----------|----------|
| 1 | `-c` / `--config` flag |
| 2 | `$XDG_CONFIG_HOME/vultr-dns-updater/config.toml` (`~/.config/…` when unset) |
| 3 | `~/.vultr-dns-updater.toml` |
| 4 | `./vultr-dns-updater.toml` |

Keys: `api_key`, `ip_check_urls`, and `[[targets]]` with `domain`, `subdomain` and `ttl`. An unknown key is ignored with a warning, so a typo like `tll = 300` doesn't go unnoticed.

### How the public IP is found

Each URL in `ip_check_urls` (or the three defaults) is asked in order, over IPv4 only, with a 10-second timeout. The first answer that is a public IPv4 address wins; private, loopback, link-local, CGNAT and other non-public answers (a captive portal, a misbehaving service) are skipped like errors. If all fail, every URL's error is printed and the exit code is 1.

## Upgrading from the Python version (0.1.0)

The Rust version is a drop-in replacement: same commands and flags, same config file, same systemd units (only the `ExecStart` path changes, to wherever the binary is), and the same output lines in the journal. On purpose, it differs in a few places:

- `ip_check_urls` from the config is used (the Python version ignored it).
- Record lists are read in full (every page), so a domain with over 100 records can't miss its target and create a duplicate.
- `update` tries every target even after one fails, and exits 1 if any failed.
- Network errors are one `Error:` line, not a traceback.
- `--ip` must be an IPv4 address.
- `status` says `Needs update` when only the TTL differs, as `update` would change it.
- `-d` and `-s` must be given together; `-t` needs them.
- `api_key` is optional in the file when `--api-key` or `VULTR_API_KEY` gives one.
- `init-config` writes the file 0600 and its directory 0700.
- Several A records with the same name: the first is updated, with a warning.
- `get-ip` takes `-c`; `status`, `list-domains` and `list-records` take `--format json`; `usage` and `self update` are new; `--version` prints `vultr-dns X.Y.Z`.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT
