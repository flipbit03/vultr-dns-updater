//! The latest GitHub release, a 24 h cache of it, semver comparison and the
//! release asset URLs. Only `self update` fetches; `usage` and `--help` read
//! the cache and never touch the network.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use serde::{Deserialize, Serialize};

const GITHUB_REPO: &str = "flipbit03/vultr-dns-updater";
const CACHE_TTL_SECS: u64 = 24 * 60 * 60;

/// The binary's name, and the prefix of every release asset.
pub const BIN_NAME: &str = "vultr-dns";

/// Overrides `https://api.github.com`. For the tests; not in `--help` or `usage`.
pub const GITHUB_API_URL_ENV: &str = "VULTR_DNS_UPDATER_GITHUB_API_URL";

/// Overrides `https://github.com/<repo>/releases/download`. For the tests;
/// not in `--help` or `usage`.
pub const RELEASES_URL_ENV: &str = "VULTR_DNS_UPDATER_RELEASES_URL";

#[derive(Debug, Serialize, Deserialize)]
struct VersionCache {
    checked_at: u64,
    latest_version: String,
}

fn env_or(var: &str, default: String) -> String {
    std::env::var(var)
        .ok()
        .filter(|v| !v.is_empty())
        .map(|v| v.trim_end_matches('/').to_string())
        .unwrap_or(default)
}

fn cache_path() -> Option<PathBuf> {
    let cache_dir = std::env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| home::home_dir().map(|h| h.join(".cache")));
    cache_dir.map(|d| {
        d.join("vultr-dns-updater")
            .join("latest_version_check.json")
    })
}

fn read_cache() -> Option<VersionCache> {
    let path = cache_path()?;
    let data = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&data).ok()
}

fn write_cache(cache: &VersionCache) {
    if let Some(path) = cache_path() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(
            &path,
            serde_json::to_string_pretty(cache).unwrap_or_default(),
        );
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// The latest (non-prerelease) release's version, without the leading `v`.
/// Always asks GitHub, and refreshes the cache.
pub async fn fetch_latest_version() -> anyhow::Result<String> {
    let api = env_or(GITHUB_API_URL_ENV, "https://api.github.com".to_string());
    let url = format!("{api}/repos/{GITHUB_REPO}/releases/latest");
    let resp: serde_json::Value = crate::http::api_client()
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("failed to reach the GitHub API")?
        .error_for_status()
        .context("the GitHub API returned an error")?
        .json()
        .await
        .context("failed to parse the GitHub API response")?;

    let tag = resp["tag_name"]
        .as_str()
        .context("no tag_name in the GitHub release")?;
    let version = tag.strip_prefix('v').unwrap_or(tag).to_string();
    write_cache(&VersionCache {
        checked_at: now_secs(),
        latest_version: version.clone(),
    });
    Ok(version)
}

/// The cached latest version, if the cache is fresh. Never fetches.
pub fn get_cached_version() -> Option<String> {
    let cache = read_cache()?;
    if now_secs().saturating_sub(cache.checked_at) < CACHE_TTL_SECS {
        Some(cache.latest_version)
    } else {
        None
    }
}

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn is_dev_build() -> bool {
    current_version() == "0.0.0"
}

pub fn is_newer(current: &str, latest: &str) -> bool {
    match (
        semver::Version::parse(current),
        semver::Version::parse(latest),
    ) {
        (Ok(c), Ok(l)) => l > c,
        _ => latest != current,
    }
}

/// `Update available: X -> Y. Run: vultr-dns self update`, from the cache
/// only, or nothing.
pub fn cached_update_hint() -> Option<String> {
    if is_dev_build() {
        return None;
    }
    let latest = get_cached_version()?;
    let current = current_version();
    is_newer(current, &latest)
        .then(|| format!("Update available: {current} -> {latest}. Run: {BIN_NAME} self update"))
}

/// This platform's asset name: `vultr-dns_<os>_<arch>`.
pub fn asset_name() -> anyhow::Result<String> {
    asset_name_for(std::env::consts::OS, std::env::consts::ARCH)
}

fn asset_name_for(os: &str, arch: &str) -> anyhow::Result<String> {
    let os_tag = match os {
        "linux" => "linux",
        "macos" => "macos",
        _ => anyhow::bail!("unsupported OS: {os}"),
    };
    let arch_tag = match arch {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        _ => anyhow::bail!("unsupported architecture: {arch}"),
    };
    if os_tag == "macos" && arch_tag == "x86_64" {
        anyhow::bail!(
            "macOS x86_64 binaries are not provided; use `cargo install vultr-dns-updater --locked`"
        );
    }
    Ok(format!("{BIN_NAME}_{os_tag}_{arch_tag}"))
}

/// Where release `version`'s asset is downloaded from. Its checksum is the
/// same URL plus `.sha256`.
pub fn release_asset_url(version: &str) -> anyhow::Result<String> {
    let base = env_or(
        RELEASES_URL_ENV,
        format!("https://github.com/{GITHUB_REPO}/releases/download"),
    );
    Ok(format!("{base}/v{version}/{}", asset_name()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_by_semver() {
        assert!(is_newer("1.0.0", "1.0.1"));
        assert!(is_newer("1.0.0", "1.1.0"));
        assert!(is_newer("1.9.0", "1.10.0"));
        assert!(!is_newer("1.0.1", "1.0.0"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(is_newer("1.0.0-rc.1", "1.0.0"));
        assert!(is_newer("1.0.0-rc.1", "1.0.0-rc.2"));
        assert!(!is_newer("1.0.0", "1.0.0-rc.9"));
        // Unparseable: anything different counts as newer.
        assert!(is_newer("1.0.0", "garbage"));
    }

    #[test]
    fn asset_names_per_platform() {
        assert_eq!(
            asset_name_for("linux", "x86_64").unwrap(),
            "vultr-dns_linux_x86_64"
        );
        assert_eq!(
            asset_name_for("linux", "aarch64").unwrap(),
            "vultr-dns_linux_aarch64"
        );
        assert_eq!(
            asset_name_for("macos", "aarch64").unwrap(),
            "vultr-dns_macos_aarch64"
        );
        assert!(asset_name_for("macos", "x86_64").is_err());
        assert!(asset_name_for("windows", "x86_64").is_err());
        assert!(asset_name_for("linux", "riscv64").is_err());
    }

    #[test]
    fn asset_url_shape() {
        let url = release_asset_url("1.2.3").unwrap();
        assert!(
            url.starts_with(
                "https://github.com/flipbit03/vultr-dns-updater/releases/download/v1.2.3/vultr-dns_"
            ) || std::env::var_os(RELEASES_URL_ENV).is_some(),
            "{url}"
        );
    }
}
