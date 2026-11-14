//! `vultr-dns usage`: the agent-facing reference (<1000 tokens, plain text).
//! `--help` is the detailed clap output for humans; this is "what can I do?".

use crate::config::{self, DEFAULT_TTL};
use crate::output::{out, outln};
use crate::service::DEFAULT_INTERVAL;
use crate::version_check;

pub fn run() {
    let version = version_check::current_version();
    let default_config = config::default_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "~/.config/vultr-dns-updater/config.toml".to_string());
    out!("{}", body(version, &default_config));
    out!("{}", config_footer(config::find().as_deref()));
    if let Some(hint) = version_check::cached_update_hint() {
        outln!();
        outln!("{hint}");
    }
}

fn body(version: &str, default_config: &str) -> String {
    format!(
        r#"vultr-dns {version} -- dynamic DNS for Vultr: points A records at this network's public IPv4

Detects the public IPv4 (first good answer from a list of services), then creates or
updates each target's A record through the Vultr API. Normally run by a systemd timer.

UPDATING:
  vultr-dns update                       Update every target in the config
  vultr-dns update --dry-run             Show what would change, change nothing
  vultr-dns update -f                    Write even if IP and TTL already match
  vultr-dns update --ip 1.2.3.4          Use this IPv4 instead of detecting it
  vultr-dns update -d DOMAIN -s SUB      One target from flags, no targets from config
    [-t TTL]                             (-d and -s go together; TTL default {DEFAULT_TTL})
  Every target is tried even if one fails; exit 1 if any failed.

READING (no changes):
  vultr-dns status [-d DOMAIN -s SUB]    Public IP and each target: Up to date / Needs
                                         update (IP or TTL differ) / Not found
  vultr-dns get-ip                       Print the detected public IPv4
  vultr-dns list-domains                 Domains in the Vultr account
  vultr-dns list-records DOMAIN          Records of one domain (Name "@" = apex)
  status, list-domains, list-records take --format json (default: human, even piped).

CONFIG:
  -c, --config PATH      Config file. Default search, first found wins:
                         {default_config}, ~/.vultr-dns-updater.toml,
                         ./vultr-dns-updater.toml
  --api-key KEY          Vultr API key; beats VULTR_API_KEY, which beats api_key in the file
  vultr-dns init-config [-p PATH]        Write the example config (0600; refuses to overwrite)
  vultr-dns show-config-example          Print it
  Keys: api_key, ip_check_urls, [[targets]] domain, subdomain ("" = apex), ttl.

SERVICE (systemd, uses sudo):
  vultr-dns service install [-i MIN]     Timer running `update` every MIN minutes (default {DEFAULT_INTERVAL})
  vultr-dns service status | uninstall
  Logs: journalctl -u vultr-dns-updater

SELF:
  vultr-dns self update [--check]        Update vultr-dns (verified download), or just check
  vultr-dns --help                       Every flag in detail
  Exit codes: 0 ok, 1 runtime error, 2 usage error.
"#
    )
}

fn config_footer(found: Option<&std::path::Path>) -> String {
    match found {
        Some(p) => format!("\nCONFIG (this machine): {}\n", p.display()),
        None => "\nCONFIG (this machine): none found. Run: vultr-dns init-config\n".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered() -> String {
        body("1.2.3", "/home/u/.config/vultr-dns-updater/config.toml")
    }

    #[test]
    fn has_every_command() {
        let text = rendered();
        for needle in [
            "vultr-dns 1.2.3",
            "UPDATING:",
            "READING",
            "CONFIG:",
            "SERVICE",
            "SELF:",
            "vultr-dns update",
            "--dry-run",
            "--ip",
            "vultr-dns status",
            "vultr-dns get-ip",
            "vultr-dns list-domains",
            "vultr-dns list-records DOMAIN",
            "--format json",
            "vultr-dns init-config",
            "vultr-dns show-config-example",
            "vultr-dns service install",
            "vultr-dns self update",
            "vultr-dns --help",
            "/home/u/.config/vultr-dns-updater/config.toml",
            "VULTR_API_KEY",
        ] {
            assert!(text.contains(needle), "missing {needle:?}");
        }
    }

    #[test]
    fn plain_text_not_markdown() {
        let text = rendered();
        assert!(!text.lines().any(|l| l.starts_with('#')));
        assert!(!text.contains("|---"));
        assert!(!text.contains("```"));
    }

    #[test]
    fn test_only_env_vars_stay_out() {
        let text = rendered();
        assert!(!text.contains("VULTR_DNS_UPDATER_"));
    }

    #[test]
    fn stays_under_token_budget() {
        let text = rendered() + &config_footer(None);
        assert!(text.len() < 4000, "usage is {} bytes", text.len());
    }
}
