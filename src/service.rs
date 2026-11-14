//! `vultr-dns service install|uninstall|status`: a system-level systemd
//! service and timer that run `vultr-dns update` as the installing user.
//!
//! The unit texts are the ones my_infra's playbook writes; a test holds them
//! byte-identical to its files as rendered on OURSERVER (`tests/fixtures/`).

use crate::output::outln;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use anyhow::{Context, Result, bail};
use colored::Colorize;

use crate::{config, output};

pub const SERVICE_NAME: &str = "vultr-dns-updater";
const SYSTEMD_DIR: &str = "/etc/systemd/system";

/// The interval `service install` uses without `--interval`, in minutes.
pub const DEFAULT_INTERVAL: u32 = 30;

pub fn service_unit(user: &str, executable: &Path, config: &Path) -> String {
    format!(
        "[Unit]
Description=Vultr DNS Updater
After=network-online.target
Wants=network-online.target

[Service]
Type=oneshot
User={user}
ExecStart={} update --config {}

[Install]
WantedBy=multi-user.target
",
        executable.display(),
        config.display()
    )
}

pub fn timer_unit(interval: u32) -> String {
    format!(
        "[Unit]
Description=Run Vultr DNS Updater every {interval} minutes

[Timer]
OnBootSec=1min
OnUnitActiveSec={interval}min
Persistent=true

[Install]
WantedBy=timers.target
"
    )
}

fn unit_paths() -> (PathBuf, PathBuf) {
    let dir = Path::new(SYSTEMD_DIR);
    (
        dir.join(format!("{SERVICE_NAME}.service")),
        dir.join(format!("{SERVICE_NAME}.timer")),
    )
}

fn sudo(args: &[&str]) -> Result<bool> {
    let status = Command::new("sudo")
        .args(args)
        .status()
        .context("could not run sudo")?;
    Ok(status.success())
}

fn systemctl_captured(args: &[&str]) -> Result<Output> {
    Command::new("systemctl")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .context("could not run systemctl")
}

/// Writes `content` to a fresh 0600 file in the temp dir.
fn temp_unit(name: &str, content: &str) -> Result<PathBuf> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let path = std::env::temp_dir().join(format!("{name}.{}", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .with_context(|| format!("could not create {}", path.display()))?;
    f.write_all(content.as_bytes())?;
    Ok(path)
}

/// Copies a temp unit into place with `sudo cp` (keeping its 0600 mode on a
/// new file), always removing the temp file.
fn install_unit(tmp: &Path, dest: &Path) -> Result<bool> {
    let ok = sudo(&["cp", &tmp.to_string_lossy(), &dest.to_string_lossy()]);
    let _ = std::fs::remove_file(tmp);
    ok
}

pub fn install(interval: u32) -> Result<()> {
    let user = std::env::var("USER").unwrap_or_default();
    if user.is_empty() || user == "root" {
        bail!("Cannot determine current user. Don't run as root.");
    }

    let loaded = match config::find() {
        None => Err(anyhow::anyhow!(config::not_found_message())),
        Some(path) => config::load(&path).and_then(|cfg| {
            let path = path
                .canonicalize()
                .with_context(|| format!("could not resolve {}", path.display()))?;
            Ok((path, cfg))
        }),
    };
    let (config_path, cfg) = match loaded {
        Ok(found) => found,
        Err(e) => bail!("Configuration required before installing service:\n{e:#}"),
    };
    if cfg.targets.is_empty() {
        bail!("No targets configured. Add targets to your config file first.");
    }

    let executable = std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .context("could not determine the path of this executable")?;
    let (service_file, timer_file) = unit_paths();

    outln!();
    outln!("{}", format!("Installing {SERVICE_NAME} service...").bold());
    outln!();
    outln!("  User: {}", user.cyan());
    outln!("  Config: {}", config_path.display().to_string().cyan());
    outln!("  Executable: {}", executable.display().to_string().cyan());
    outln!("  Interval: {}", format!("{interval} minutes").cyan());
    outln!();

    let temp_service = temp_unit(
        &format!("{SERVICE_NAME}.service"),
        &service_unit(&user, &executable, &config_path),
    )?;
    let temp_timer = match temp_unit(&format!("{SERVICE_NAME}.timer"), &timer_unit(interval)) {
        Ok(p) => p,
        Err(e) => {
            let _ = std::fs::remove_file(&temp_service);
            return Err(e);
        }
    };

    outln!("{}", "sudo password may be required...".dimmed());
    outln!();

    if !install_unit(&temp_service, &service_file)? {
        let _ = std::fs::remove_file(&temp_timer);
        bail!("Failed to install service file. Is sudo available?");
    }
    outln!("  Created: {}", service_file.display());

    if !install_unit(&temp_timer, &timer_file)? {
        bail!("Failed to install timer file.");
    }
    outln!("  Created: {}", timer_file.display());

    outln!();
    outln!("{}", "Enabling service...".bold());
    outln!();

    sudo(&["systemctl", "daemon-reload"])?;
    outln!("  Reloaded systemd daemon");

    let timer = format!("{SERVICE_NAME}.timer");
    if !sudo(&["systemctl", "enable", &timer])? {
        bail!("Failed to enable timer");
    }
    outln!("  Enabled {timer}");

    if !sudo(&["systemctl", "start", &timer])? {
        bail!("Failed to start timer");
    }
    outln!("  Started {timer}");

    outln!();
    output::success(&format!(
        "Service installed! DNS will update every {interval} minutes."
    ));
    outln!();
    outln!(
        "{}",
        format!("The service runs as user '{user}' but is managed at system level.").dimmed()
    );
    outln!(
        "{}  journalctl -u {SERVICE_NAME}.service",
        "View logs:".dimmed()
    );
    outln!("{} systemctl list-timers {timer}", "Check timer:".dimmed());
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let (service_file, timer_file) = unit_paths();
    if !service_file.exists() && !timer_file.exists() {
        output::info("No service installed. Nothing to uninstall.");
        return Ok(());
    }

    let timer = format!("{SERVICE_NAME}.timer");
    outln!();
    outln!(
        "{}",
        format!("Uninstalling {SERVICE_NAME} service...").bold()
    );
    outln!();
    outln!("{}", "sudo password may be required...".dimmed());
    outln!();

    sudo(&["systemctl", "stop", &timer])?;
    outln!("  Stopped {timer}");
    sudo(&["systemctl", "disable", &timer])?;
    outln!("  Disabled {timer}");

    for file in [&timer_file, &service_file] {
        if file.exists() {
            if !sudo(&["rm", &file.to_string_lossy()])? {
                bail!("Failed to remove {}", file.display());
            }
            outln!("  Removed: {}", file.display());
        }
    }

    sudo(&["systemctl", "daemon-reload"])?;
    outln!("  Reloaded systemd daemon");

    outln!();
    output::success("Service uninstalled successfully.");
    Ok(())
}

pub fn status() -> Result<()> {
    let timer = format!("{SERVICE_NAME}.timer");
    outln!();
    outln!("{}", format!("{SERVICE_NAME} Timer Status:").bold());
    outln!();

    let out = systemctl_captured(&["status", &timer])?;
    if out.status.code() == Some(4) {
        output::info("No service installed.\nInstall with: vultr-dns service install");
        return Ok(());
    }
    print_output(&out);

    outln!();
    outln!("{}", "Scheduled Timers:".bold());
    outln!();
    print_output(&systemctl_captured(&["list-timers", &timer])?);
    Ok(())
}

fn print_output(out: &Output) {
    let text = if out.stdout.is_empty() {
        &out.stderr
    } else {
        &out.stdout
    };
    outln!("{}", String::from_utf8_lossy(text).trim_end());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// my_infra's `rustible/playbooks/ourserver/files/vultr-dns-updater.service.j2`
    /// as the playbook renders it on OURSERVER (user `cadu`, home `/home/cadu`),
    /// with the binary installed by install.sh in `~/.local/bin`.
    const PLAYBOOK_SERVICE: &str = include_str!("../tests/fixtures/vultr-dns-updater.service");

    #[test]
    fn service_unit_matches_my_infra() {
        assert_eq!(
            service_unit(
                "cadu",
                Path::new("/home/cadu/.local/bin/vultr-dns"),
                Path::new("/home/cadu/.config/vultr-dns-updater/config.toml"),
            ),
            PLAYBOOK_SERVICE
        );
        assert!(PLAYBOOK_SERVICE.contains(
            "\nExecStart=/home/cadu/.local/bin/vultr-dns update --config /home/cadu/.config/vultr-dns-updater/config.toml\n"
        ));
    }

    #[test]
    fn timer_unit_matches_my_infra() {
        assert_eq!(
            timer_unit(DEFAULT_INTERVAL),
            include_str!("../tests/fixtures/vultr-dns-updater.timer")
        );
    }

    #[test]
    fn timer_interval() {
        let t = timer_unit(15);
        assert!(t.contains("OnUnitActiveSec=15min\n"));
        assert!(t.contains("every 15 minutes\n"));
    }
}
