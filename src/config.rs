//! The config file: search order, loading, validation, the example text and
//! `init-config`.
//!
//! The file format is the Python tool's, unchanged: `api_key`, `[[targets]]`
//! (`domain`, `subdomain`, `ttl`) and `ip_check_urls`. Unknown keys are
//! accepted, with a warning each.

use std::ffi::OsString;
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::output;

/// The text `init-config` writes and `show-config-example` prints. Kept
/// verbatim from the Python tool.
pub const EXAMPLE_CONFIG: &str = r#"# Vultr DNS Updater Configuration
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
"#;

/// Where the public IP is asked for when the config names no `ip_check_urls`.
pub const DEFAULT_IP_CHECK_URLS: [&str; 3] = [
    "https://api.ipify.org",
    "https://ifconfig.me/ip",
    "https://icanhazip.com",
];

/// A target's TTL when the config (or `--ttl`) doesn't give one.
pub const DEFAULT_TTL: u32 = 60;

/// What every command that needs Vultr says when no source gave a key.
pub const NO_API_KEY: &str =
    "No API key provided. Use --api-key, set VULTR_API_KEY, or add to config";

const APP_DIR: &str = "vultr-dns-updater";

/// The Vultr API key. Its `Debug` never shows the value, so it can't leak
/// through an error or a debug print.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

/// One record to keep pointed at the public IP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub domain: String,
    /// The record name relative to `domain`; empty means the apex.
    pub subdomain: String,
    pub ttl: u32,
}

impl Target {
    pub fn fqdn(&self) -> String {
        if self.subdomain.is_empty() {
            self.domain.clone()
        } else {
            format!("{}.{}", self.subdomain, self.domain)
        }
    }
}

/// A loaded, validated config file.
#[derive(Debug)]
pub struct Config {
    pub api_key: Option<Secret>,
    pub targets: Vec<Target>,
    pub ip_check_urls: Vec<String>,
}

impl Config {
    /// The URLs to ask for the public IP, in order.
    pub fn ip_check_urls(config: Option<&Config>) -> Vec<String> {
        match config {
            Some(c) if !c.ip_check_urls.is_empty() => c.ip_check_urls.clone(),
            _ => DEFAULT_IP_CHECK_URLS
                .iter()
                .map(|u| u.to_string())
                .collect(),
        }
    }
}

#[derive(Deserialize)]
struct RawConfig {
    api_key: Option<String>,
    #[serde(default)]
    targets: Vec<RawTarget>,
    ip_check_urls: Option<Vec<String>>,
}

/// One `[[targets]]` entry as written. A later `type = "AAAA"` would be one
/// more optional field here, defaulting to `"A"`; existing files keep parsing.
#[derive(Deserialize)]
struct RawTarget {
    domain: String,
    subdomain: String,
    #[serde(default = "default_ttl")]
    ttl: i64,
}

fn default_ttl() -> i64 {
    DEFAULT_TTL.into()
}

/// `$XDG_CONFIG_HOME/vultr-dns-updater/config.toml`, else `~/.config/…`.
pub fn default_path() -> Option<PathBuf> {
    default_path_from(std::env::var_os("XDG_CONFIG_HOME"), home::home_dir())
}

fn default_path_from(xdg: Option<OsString>, home: Option<PathBuf>) -> Option<PathBuf> {
    let base = xdg
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.map(|h| h.join(".config")))?;
    Some(base.join(APP_DIR).join("config.toml"))
}

/// Where a config is looked for when `--config` isn't given, first match wins.
pub fn search_paths() -> Vec<PathBuf> {
    search_paths_from(std::env::var_os("XDG_CONFIG_HOME"), home::home_dir())
}

fn search_paths_from(xdg: Option<OsString>, home: Option<PathBuf>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(p) = default_path_from(xdg, home.clone()) {
        paths.push(p);
    }
    if let Some(h) = home {
        paths.push(h.join(".vultr-dns-updater.toml"));
    }
    paths.push(PathBuf::from("vultr-dns-updater.toml"));
    paths
}

/// The first config file the search order finds.
pub fn find() -> Option<PathBuf> {
    search_paths().into_iter().find(|p| p.is_file())
}

/// Python's "No configuration file found" text, listing the search order.
pub fn not_found_message() -> String {
    let searched: Vec<String> = search_paths()
        .iter()
        .map(|p| format!("  - {}", p.display()))
        .collect();
    format!(
        "No configuration file found. Searched:\n{}\n\nCreate a config file with:\n  vultr-dns init-config",
        searched.join("\n")
    )
}

/// `--config` if given, else the search order; no file at all is an error.
pub fn load_required(explicit: Option<&Path>) -> Result<Config> {
    match explicit.map(Path::to_path_buf).or_else(find) {
        Some(path) => load(&path),
        None => bail!(not_found_message()),
    }
}

/// `--config` if given, else the search order; no file at all is fine.
pub fn load_optional(explicit: Option<&Path>) -> Result<Option<Config>> {
    match explicit.map(Path::to_path_buf).or_else(find) {
        Some(path) => load(&path).map(Some),
        None => Ok(None),
    }
}

/// Reads, parses and validates one file, warning about unknown keys.
pub fn load(path: &Path) -> Result<Config> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("could not read config file {}", path.display()))?;
    let (config, unknown) = parse(&text, path)?;
    for key in unknown {
        output::warning(&format!(
            "unknown config key \"{key}\" in {}",
            path.display()
        ));
    }
    Ok(config)
}

/// Parses and validates config text. Returns the config and the unknown keys.
fn parse(text: &str, path: &Path) -> Result<(Config, Vec<String>)> {
    let invalid = |msg: &dyn fmt::Display| {
        anyhow::anyhow!("Invalid configuration in {}: {msg}", path.display())
    };

    let mut unknown = Vec::new();
    let raw: RawConfig = serde_ignored::deserialize(toml::Deserializer::new(text), |p| {
        unknown.push(p.to_string())
    })
    .map_err(|e| invalid(&e.to_string().trim_end()))?;

    let mut targets = Vec::with_capacity(raw.targets.len());
    for (i, t) in raw.targets.into_iter().enumerate() {
        if t.domain.trim().is_empty() {
            return Err(invalid(&format!("targets[{i}].domain must not be empty")));
        }
        let ttl = u32::try_from(t.ttl)
            .ok()
            .filter(|&ttl| ttl > 0)
            .ok_or_else(|| {
                invalid(&format!(
                    "targets[{i}].ttl must be a positive integer (got {})",
                    t.ttl
                ))
            })?;
        targets.push(Target {
            domain: t.domain,
            subdomain: t.subdomain,
            ttl,
        });
    }

    let ip_check_urls = raw.ip_check_urls.unwrap_or_default();
    for (i, url) in ip_check_urls.iter().enumerate() {
        let ok = reqwest::Url::parse(url)
            .map(|u| matches!(u.scheme(), "http" | "https") && u.host().is_some())
            .unwrap_or(false);
        if !ok {
            return Err(invalid(&format!(
                "ip_check_urls[{i}] must be an http:// or https:// URL (got \"{url}\")"
            )));
        }
    }

    let config = Config {
        api_key: raw.api_key.filter(|k| !k.is_empty()).map(Secret::new),
        targets,
        ip_check_urls,
    };
    Ok((config, unknown))
}

/// The key to use: `--api-key` / `VULTR_API_KEY` beats the config file.
pub fn resolve_api_key(flag: Option<&str>, config: Option<&Config>) -> Result<Secret> {
    if let Some(key) = flag.filter(|k| !k.is_empty()) {
        return Ok(Secret::new(key));
    }
    config
        .and_then(|c| c.api_key.clone())
        .ok_or_else(|| anyhow::anyhow!(NO_API_KEY))
}

/// Writes the example config: the directories it creates are 0700, the
/// file 0600 (it will hold an API key). Refuses to overwrite.
pub fn init_config(path: Option<PathBuf>) -> Result<PathBuf> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

    let path = match path {
        Some(p) => p,
        None => default_path().context("could not determine the home directory")?,
    };
    if path.exists() {
        bail!("Configuration file already exists: {}", path.display());
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .with_context(|| format!("could not create {}", path.display()))?;
    file.write_all(EXAMPLE_CONFIG.as_bytes())
        .with_context(|| format!("could not write {}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(text: &str) -> (Config, Vec<String>) {
        parse(text, Path::new("/x/config.toml")).unwrap()
    }

    fn parse_err(text: &str) -> String {
        format!(
            "{:#}",
            parse(text, Path::new("/x/config.toml")).unwrap_err()
        )
    }

    #[test]
    fn example_with_values_parses() {
        // OURSERVER's file: the example text with the key and one target filled in.
        let text = EXAMPLE_CONFIG
            .replace("YOUR_VULTR_API_KEY", "k-123")
            .replace("example.com\"      #", "flipbit03.com\"    #");
        let (cfg, unknown) = parse_ok(&text);
        assert!(unknown.is_empty(), "{unknown:?}");
        assert_eq!(cfg.api_key, Some(Secret::new("k-123")));
        assert_eq!(
            cfg.targets,
            vec![Target {
                domain: "flipbit03.com".into(),
                subdomain: "home".into(),
                ttl: 60
            }]
        );
        assert_eq!(cfg.targets[0].fqdn(), "home.flipbit03.com");
        assert!(cfg.ip_check_urls.is_empty());
        assert_eq!(Config::ip_check_urls(Some(&cfg)), DEFAULT_IP_CHECK_URLS);
    }

    #[test]
    fn apex_and_default_ttl() {
        let (cfg, _) = parse_ok("[[targets]]\ndomain = \"example.com\"\nsubdomain = \"\"\n");
        assert_eq!(cfg.targets[0].ttl, DEFAULT_TTL);
        assert_eq!(cfg.targets[0].fqdn(), "example.com");
        assert_eq!(cfg.api_key, None);
    }

    #[test]
    fn api_key_is_optional_and_empty_means_none() {
        let (cfg, _) = parse_ok("api_key = \"\"\n");
        assert_eq!(cfg.api_key, None);
        assert!(cfg.targets.is_empty());
    }

    #[test]
    fn unknown_keys_are_reported() {
        let (cfg, unknown) = parse_ok(
            "api_key = \"k\"\nfoo = 1\n[[targets]]\ndomain = \"a.com\"\nsubdomain = \"h\"\ntll = 300\n",
        );
        assert_eq!(cfg.targets[0].ttl, 60);
        assert_eq!(
            unknown,
            vec!["foo".to_string(), "targets.0.tll".to_string()]
        );
    }

    #[test]
    fn bad_ttl_names_the_key() {
        for ttl in ["0", "-5", "5000000000"] {
            let err = parse_err(&format!(
                "[[targets]]\ndomain = \"a.com\"\nsubdomain = \"h\"\nttl = {ttl}\n"
            ));
            assert!(
                err.contains("Invalid configuration in /x/config.toml"),
                "{err}"
            );
            assert!(
                err.contains("targets[0].ttl must be a positive integer"),
                "{err}"
            );
        }
        let err = parse_err("[[targets]]\ndomain = \"a.com\"\nsubdomain = \"h\"\nttl = \"x\"\n");
        assert!(
            err.contains("Invalid configuration in /x/config.toml"),
            "{err}"
        );
        assert!(err.contains("ttl"), "{err}");
    }

    #[test]
    fn empty_domain_and_missing_subdomain_error() {
        let err = parse_err("[[targets]]\ndomain = \" \"\nsubdomain = \"h\"\n");
        assert!(err.contains("targets[0].domain must not be empty"), "{err}");
        let err = parse_err("[[targets]]\ndomain = \"a.com\"\n");
        assert!(err.contains("subdomain"), "{err}");
    }

    #[test]
    fn ip_check_urls_must_be_http() {
        let (cfg, _) = parse_ok("ip_check_urls = [\"http://127.0.0.1:1/ip\", \"https://x.io\"]\n");
        assert_eq!(cfg.ip_check_urls.len(), 2);
        assert_eq!(
            Config::ip_check_urls(Some(&cfg)),
            vec!["http://127.0.0.1:1/ip", "https://x.io"]
        );
        let err = parse_err("ip_check_urls = [\"ftp://x.io\"]\n");
        assert!(err.contains("ip_check_urls[0]"), "{err}");
        let err = parse_err("ip_check_urls = [\"not a url\"]\n");
        assert!(err.contains("ip_check_urls[0]"), "{err}");
    }

    #[test]
    fn syntax_error_names_the_file() {
        let err = parse_err("api_key = \n");
        assert!(
            err.starts_with("Invalid configuration in /x/config.toml: "),
            "{err}"
        );
    }

    #[test]
    fn search_order() {
        let home = PathBuf::from("/home/u");
        assert_eq!(
            search_paths_from(None, Some(home.clone())),
            vec![
                PathBuf::from("/home/u/.config/vultr-dns-updater/config.toml"),
                PathBuf::from("/home/u/.vultr-dns-updater.toml"),
                PathBuf::from("vultr-dns-updater.toml"),
            ]
        );
        assert_eq!(
            search_paths_from(Some("/xdg".into()), Some(home.clone()))[0],
            PathBuf::from("/xdg/vultr-dns-updater/config.toml")
        );
        // An empty XDG_CONFIG_HOME counts as unset.
        assert_eq!(
            search_paths_from(Some("".into()), Some(home))[0],
            PathBuf::from("/home/u/.config/vultr-dns-updater/config.toml")
        );
        assert_eq!(
            search_paths_from(None, None),
            vec![PathBuf::from("vultr-dns-updater.toml")]
        );
    }

    #[test]
    fn key_precedence() {
        let (cfg, _) = parse_ok("api_key = \"from-file\"\n");
        let key = |flag, cfg| resolve_api_key(flag, cfg).map(|s| s.expose().to_string());
        assert_eq!(key(Some("from-flag"), Some(&cfg)).unwrap(), "from-flag");
        assert_eq!(key(Some(""), Some(&cfg)).unwrap(), "from-file");
        assert_eq!(key(None, Some(&cfg)).unwrap(), "from-file");
        assert_eq!(key(Some("from-flag"), None).unwrap(), "from-flag");
        let err = key(None, None).unwrap_err().to_string();
        assert_eq!(err, NO_API_KEY);
        let (empty, _) = parse_ok("");
        assert_eq!(key(None, Some(&empty)).unwrap_err().to_string(), NO_API_KEY);
    }

    #[test]
    fn secret_debug_is_masked() {
        let s = Secret::new("super-secret");
        assert_eq!(format!("{s:?}"), "***");
        let cfg = Config {
            api_key: Some(s),
            targets: vec![],
            ip_check_urls: vec![],
        };
        assert!(!format!("{cfg:?}").contains("super-secret"));
    }

    #[test]
    fn init_config_modes_and_refusal() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/b/config.toml");
        init_config(Some(path.clone())).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), EXAMPLE_CONFIG);
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(&dir.path().join("a/b")), 0o700);
        assert_eq!(mode(&dir.path().join("a")), 0o700);
        let err = init_config(Some(path.clone())).unwrap_err().to_string();
        assert!(
            err.starts_with("Configuration file already exists: "),
            "{err}"
        );
    }
}
