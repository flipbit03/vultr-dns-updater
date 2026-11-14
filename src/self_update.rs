//! `vultr-dns self update [--check]`.
//!
//! A release binary (built with `binary-release`) downloads the new asset
//! and its `.sha256`, refuses on a mismatch, smoke-tests the new binary,
//! keeps the old one as `vultr-dns.old` and swaps atomically. A crates.io
//! build runs `cargo install` again. Never runs on its own: the timer's
//! `update` doesn't check for updates.

use crate::output::outln;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use colored::Colorize;
use sha2::{Digest, Sha256};

use crate::version_check::{self, BIN_NAME};

#[derive(Debug, Parser)]
pub struct SelfCmd {
    #[command(subcommand)]
    pub action: SelfAction,
}

#[derive(Debug, Subcommand)]
pub enum SelfAction {
    /// Update vultr-dns to the latest release.
    Update(UpdateArgs),
}

#[derive(Debug, Parser)]
pub struct UpdateArgs {
    /// Only check whether an update is available; don't install it.
    #[arg(long)]
    check: bool,
}

pub async fn run(cmd: SelfCmd) -> Result<()> {
    match cmd.action {
        SelfAction::Update(args) => run_update(args).await,
    }
}

async fn run_update(args: UpdateArgs) -> Result<()> {
    if version_check::is_dev_build() {
        bail!(
            "this is a dev build (0.0.0), which can't update itself. \
             Install a release instead (install.sh, or `cargo install vultr-dns-updater --locked`)."
        );
    }

    let current = version_check::current_version();
    let latest = version_check::fetch_latest_version()
        .await
        .context("could not determine the latest version")?;

    if !version_check::is_newer(current, &latest) {
        outln!(
            "{BIN_NAME} {} is already the latest version.",
            current.green()
        );
        return Ok(());
    }
    if args.check {
        outln!(
            "Update available: {} -> {}",
            current.yellow(),
            latest.green()
        );
        outln!("Run `{BIN_NAME} self update` to upgrade.");
        return Ok(());
    }

    outln!(
        "Updating {BIN_NAME} {} -> {}",
        current.yellow(),
        latest.green()
    );
    if cfg!(feature = "binary-release") {
        update_binary(&latest).await
    } else {
        update_cargo()
    }
}

async fn update_binary(version: &str) -> Result<()> {
    let exe = std::env::current_exe().context("could not determine the current executable")?;
    let exe = exe.canonicalize().unwrap_or(exe);
    let url = version_check::release_asset_url(version)?;
    let old = download_and_install(&exe, &url, version).await?;
    outln!(
        "{BIN_NAME} {} installed to {}",
        version.green(),
        exe.display()
    );
    outln!(
        "The previous binary is kept at {} (to roll back: mv {} {})",
        old.display(),
        old.display(),
        exe.display()
    );
    Ok(())
}

/// Checks `exe`'s directory is writable, downloads `url` and `url.sha256`,
/// then `install_verified`. Returns the path of the `.old` copy.
async fn download_and_install(exe: &Path, url: &str, version: &str) -> Result<PathBuf> {
    let dir = exe
        .parent()
        .context("the current executable has no parent directory")?;
    check_writable(dir)?;

    outln!("Downloading {url}");
    let client = crate::http::download_client();
    let bytes = download(&client, url)
        .await
        .context("failed to download the release binary")?;
    let sidecar_url = format!("{url}.sha256");
    let sidecar = download(&client, &sidecar_url).await.with_context(|| {
        format!("failed to download {sidecar_url}; not installing an unverified binary")
    })?;
    let sidecar = String::from_utf8(sidecar).context("the .sha256 file isn't text")?;
    install_verified(exe, &bytes, &sidecar, version)
}

async fn download(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let bytes = client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    Ok(bytes.to_vec())
}

fn update_cargo() -> Result<()> {
    let has_cargo = std::process::Command::new("cargo")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !has_cargo {
        bail!(
            "{BIN_NAME} was installed with cargo, but `cargo` is not in your PATH.\n\
             Install Rust from https://rustup.rs or add cargo to your PATH."
        );
    }

    outln!("Running `cargo install vultr-dns-updater --locked`...");
    let status = std::process::Command::new("cargo")
        .args(["install", "vultr-dns-updater", "--locked"])
        .status()
        .context("failed to run `cargo install vultr-dns-updater --locked`")?;
    if !status.success() {
        bail!("`cargo install vultr-dns-updater --locked` exited with {status}");
    }
    outln!("{}", "Update complete.".green());
    Ok(())
}

/// Fails, before anything is downloaded, when `dir` can't take the new binary.
fn check_writable(dir: &Path) -> Result<()> {
    let probe = dir.join(format!(".{BIN_NAME}.write-test-{}", std::process::id()));
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            Ok(())
        }
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem
            ) =>
        {
            let user = std::env::var("USER")
                .ok()
                .filter(|u| !u.is_empty())
                .unwrap_or_else(|| "this user".to_string());
            bail!(
                "{} isn't writable by {user}: update it the way it was installed (or run with sudo).",
                dir.display()
            )
        }
        Err(e) => Err(e).with_context(|| format!("could not write to {}", dir.display())),
    }
}

/// The SHA-256 a `sha256sum` line (`<hex>  <name>`) names.
fn parse_sidecar(sidecar: &str) -> Result<String> {
    let hex = sidecar
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("the .sha256 file doesn't hold a SHA-256; not installing an unverified binary");
    }
    Ok(hex)
}

/// Removes the temp file on drop unless it was moved into place.
struct TempFile(Option<PathBuf>);

impl Drop for TempFile {
    fn drop(&mut self) {
        if let Some(p) = self.0.take() {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Verifies `bytes` against `sidecar`, writes them next to `exe`, checks
/// that the new binary reports `version`, keeps the current binary as
/// `<exe>.old` and renames the new one over `exe`. On any failure the
/// current binary is untouched and the temp file is gone. Returns the path
/// of the `.old` copy.
fn install_verified(exe: &Path, bytes: &[u8], sidecar: &str, version: &str) -> Result<PathBuf> {
    let expected = parse_sidecar(sidecar)?;
    let actual = format!("{:x}", Sha256::digest(bytes));
    if actual != expected {
        bail!(
            "checksum mismatch: the download's SHA-256 is {actual}, the release says {expected}; not installing it"
        );
    }

    let dir = exe
        .parent()
        .context("the executable has no parent directory")?;
    let file_name = exe
        .file_name()
        .context("the executable has no file name")?
        .to_string_lossy()
        .into_owned();
    let tmp_path = dir.join(format!(".{BIN_NAME}.update-{}", std::process::id()));
    let mut tmp = TempFile(Some(tmp_path.clone()));
    {
        use std::os::unix::fs::OpenOptionsExt;
        // A leftover from a crashed run with the same pid goes first; then
        // create_new, so the path can't be a planted file or symlink.
        let _ = std::fs::remove_file(&tmp_path);
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o755)
            .open(&tmp_path)
            .with_context(|| format!("could not create {}", tmp_path.display()))?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o755))?;
    }

    smoke_test(&tmp_path, version)?;

    let old = dir.join(format!("{file_name}.old"));
    let _ = std::fs::remove_file(&old);
    if std::fs::hard_link(exe, &old).is_err() {
        std::fs::copy(exe, &old)
            .with_context(|| format!("could not keep the current binary as {}", old.display()))?;
    }
    std::fs::rename(&tmp_path, exe)
        .with_context(|| format!("could not replace {}", exe.display()))?;
    tmp.0 = None;
    // Make the rename itself durable.
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(old)
}

/// Runs `<bin> --version` and requires `vultr-dns <version>`.
fn smoke_test(bin: &Path, version: &str) -> Result<()> {
    let expected = format!("{BIN_NAME} {version}");
    let mut attempt = 0;
    let out = loop {
        match std::process::Command::new(bin).arg("--version").output() {
            // Another thread may still hold a write handle from a fork in
            // flight; give it a moment.
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy && attempt < 20 => {
                attempt += 1;
                std::thread::sleep(Duration::from_millis(50));
            }
            other => break other,
        }
    }
    .context("the downloaded binary doesn't run here; not installing it")?;
    let printed = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || printed != expected {
        bail!(
            "the downloaded binary failed its smoke test (`--version` exited with {} and printed \"{}\", expected \"{expected}\"); not installing it",
            out.status,
            crate::output::excerpt(&printed, 200)
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn script(version_line: &str) -> Vec<u8> {
        format!("#!/bin/sh\necho \"{version_line}\"\n").into_bytes()
    }

    fn sidecar_for(bytes: &[u8]) -> String {
        format!("{:x}  vultr-dns_linux_x86_64\n", Sha256::digest(bytes))
    }

    fn setup() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("vultr-dns");
        std::fs::write(&exe, b"old binary").unwrap();
        (dir, exe)
    }

    fn leftovers(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with('.'))
            .collect()
    }

    #[test]
    fn good_binary_is_swapped_in_and_old_kept() {
        let (dir, exe) = setup();
        let new = script("vultr-dns 9.9.9");
        let old = install_verified(&exe, &new, &sidecar_for(&new), "9.9.9").unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), new);
        assert_eq!(old, dir.path().join("vultr-dns.old"));
        assert_eq!(std::fs::read(&old).unwrap(), b"old binary");
        let mode = std::fs::metadata(&exe).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o755);
        assert!(leftovers(dir.path()).is_empty());

        // A second update replaces the older .old.
        let newer = script("vultr-dns 9.9.10");
        install_verified(&exe, &newer, &sidecar_for(&newer), "9.9.10").unwrap();
        assert_eq!(std::fs::read(&old).unwrap(), new);
    }

    #[test]
    fn bad_checksum_refuses() {
        let (dir, exe) = setup();
        let new = script("vultr-dns 9.9.9");
        let err = install_verified(&exe, &new, &sidecar_for(b"something else"), "9.9.9")
            .unwrap_err()
            .to_string();
        assert!(err.contains("checksum mismatch"), "{err}");
        assert_eq!(std::fs::read(&exe).unwrap(), b"old binary");
        assert!(!dir.path().join("vultr-dns.old").exists());
        assert!(leftovers(dir.path()).is_empty());
    }

    #[test]
    fn missing_or_garbage_sidecar_refuses() {
        let (_dir, exe) = setup();
        let new = script("vultr-dns 9.9.9");
        for sidecar in ["", "Not Found", "abc  vultr-dns_linux_x86_64"] {
            let err = install_verified(&exe, &new, sidecar, "9.9.9")
                .unwrap_err()
                .to_string();
            assert!(err.contains("unverified"), "{sidecar:?}: {err}");
        }
        assert_eq!(std::fs::read(&exe).unwrap(), b"old binary");
    }

    #[test]
    fn binary_failing_smoke_test_is_not_installed() {
        let (dir, exe) = setup();
        for new in [
            b"#!/bin/sh\nexit 1\n".to_vec(),
            script("vultr-dns 1.0.0"),
            b"\x7fELF truncated".to_vec(),
        ] {
            let err = install_verified(&exe, &new, &sidecar_for(&new), "9.9.9")
                .unwrap_err()
                .to_string();
            assert!(err.contains("not installing it"), "{err}");
            assert_eq!(std::fs::read(&exe).unwrap(), b"old binary");
            assert!(!dir.path().join("vultr-dns.old").exists());
            assert!(
                leftovers(dir.path()).is_empty(),
                "{:?}",
                leftovers(dir.path())
            );
        }
    }

    async fn serve(asset: Option<Vec<u8>>, sidecar: Option<String>) -> wiremock::MockServer {
        use wiremock::matchers::path;
        use wiremock::{Mock, ResponseTemplate};
        let server = wiremock::MockServer::start().await;
        let found = |body: Vec<u8>| ResponseTemplate::new(200).set_body_bytes(body);
        let missing = ResponseTemplate::new(404);
        Mock::given(path("/v9.9.9/asset"))
            .respond_with(asset.map_or(missing.clone(), found))
            .mount(&server)
            .await;
        Mock::given(path("/v9.9.9/asset.sha256"))
            .respond_with(sidecar.map_or(missing, |s| found(s.into_bytes())))
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn download_path_installs_a_verified_binary() {
        let (dir, exe) = setup();
        let new = script("vultr-dns 9.9.9");
        let server = serve(Some(new.clone()), Some(sidecar_for(&new))).await;
        let url = format!("{}/v9.9.9/asset", server.uri());
        let old = download_and_install(&exe, &url, "9.9.9").await.unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), new);
        assert_eq!(std::fs::read(old).unwrap(), b"old binary");
        assert!(leftovers(dir.path()).is_empty());
    }

    #[tokio::test]
    async fn download_path_refuses_without_a_sidecar() {
        let (dir, exe) = setup();
        let new = script("vultr-dns 9.9.9");
        let server = serve(Some(new), None).await;
        let url = format!("{}/v9.9.9/asset", server.uri());
        let err = format!(
            "{:#}",
            download_and_install(&exe, &url, "9.9.9").await.unwrap_err()
        );
        assert!(
            err.contains("asset.sha256; not installing an unverified binary"),
            "{err}"
        );
        assert!(err.contains("404"), "{err}");
        assert_eq!(std::fs::read(&exe).unwrap(), b"old binary");
        assert!(leftovers(dir.path()).is_empty());
    }

    #[tokio::test]
    async fn download_path_network_error_changes_nothing() {
        let (dir, exe) = setup();
        let err = format!(
            "{:#}",
            download_and_install(&exe, "http://127.0.0.1:9/v9.9.9/asset", "9.9.9")
                .await
                .unwrap_err()
        );
        assert!(
            err.starts_with("failed to download the release binary: "),
            "{err}"
        );
        assert_eq!(std::fs::read(&exe).unwrap(), b"old binary");
        assert!(!dir.path().join("vultr-dns.old").exists());
        assert!(leftovers(dir.path()).is_empty());
    }

    #[test]
    fn stale_temp_file_is_replaced_not_followed() {
        let (dir, exe) = setup();
        let target = dir.path().join("victim");
        std::fs::write(&target, b"keep me").unwrap();
        let tmp = dir
            .path()
            .join(format!(".{BIN_NAME}.update-{}", std::process::id()));
        std::os::unix::fs::symlink(&target, &tmp).unwrap();
        let new = script("vultr-dns 9.9.9");
        install_verified(&exe, &new, &sidecar_for(&new), "9.9.9").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"keep me");
        assert_eq!(std::fs::read(&exe).unwrap(), new);
    }

    #[test]
    fn read_only_dir_gets_the_generic_message() {
        let dir = tempfile::tempdir().unwrap();
        let ro = dir.path().join("bin");
        std::fs::create_dir(&ro).unwrap();
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();
        let result = check_writable(&ro);
        let probe_worked = std::fs::write(ro.join("probe"), b"").is_ok();
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755)).unwrap();
        if probe_worked {
            // Running as root: permissions don't apply, nothing to test.
            return;
        }
        let err = result.unwrap_err().to_string();
        assert!(
            err.starts_with(&format!("{} isn't writable by ", ro.display())),
            "{err}"
        );
        assert!(
            err.ends_with(": update it the way it was installed (or run with sudo)."),
            "{err}"
        );
        assert!(check_writable(dir.path()).is_ok());
        assert!(leftovers(dir.path()).is_empty());
    }
}
