//! `vultr-dns usage` with no config anywhere (no network).

use std::process::Command;

fn vultr_dns(args: &[&str]) -> (String, String, bool) {
    let dir = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_vultr-dns"))
        .args(args)
        .current_dir(dir.path())
        .env("HOME", dir.path())
        .env("XDG_CONFIG_HOME", dir.path().join("config"))
        .env("XDG_CACHE_HOME", dir.path().join("cache"))
        .env_remove("VULTR_API_KEY")
        .output()
        .unwrap();
    (
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
        out.status.success(),
    )
}

#[test]
fn usage_works_without_a_config_and_stays_small() {
    let (stdout, stderr, ok) = vultr_dns(&["usage"]);
    assert!(ok, "{stderr}");
    assert!(stdout.contains("UPDATING:"));
    assert!(stdout.contains("vultr-dns self update"));
    assert!(
        stdout.contains("CONFIG (this machine): none found"),
        "{stdout}"
    );
    assert!(stdout.len() < 4000, "usage is {} bytes", stdout.len());
}

#[test]
fn usage_is_not_help() {
    let (usage, _, _) = vultr_dns(&["usage"]);
    let (help, _, ok) = vultr_dns(&["--help"]);
    assert!(ok);
    assert_ne!(usage, help);
    assert!(help.contains("Usage: vultr-dns <COMMAND>"));
}

#[test]
fn version_is_clap_style() {
    let (stdout, _, ok) = vultr_dns(&["--version"]);
    assert!(ok);
    assert_eq!(stdout, format!("vultr-dns {}\n", env!("CARGO_PKG_VERSION")));
}
