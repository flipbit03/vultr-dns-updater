//! `vultr-dns`: dynamic DNS for Vultr DNS.

mod config;
mod http;
mod ip;
mod output;
mod self_update;
mod service;
mod sync;
mod usage;
mod version_check;
mod vultr;

use crate::output::outln;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::{Args, Parser, Subcommand};
use serde::Serialize;
use tabled::Tabled;

use crate::config::{Config, DEFAULT_TTL, Target};
use crate::output::Format;

/// Vultr DNS Updater - Dynamic DNS for Vultr DNS service.
#[derive(Debug, Parser)]
#[command(
    name = "vultr-dns",
    version,
    about,
    after_help = version_check::cached_update_hint().unwrap_or_default()
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Update DNS records with the current public IP.
    ///
    /// Uses configuration file targets, or specify --domain and --subdomain.
    Update(UpdateArgs),
    /// Check the current status of DNS records vs public IP (IP and TTL).
    Status(StatusArgs),
    /// Get the current public IP address.
    GetIp(ConfigArg),
    /// List all DNS domains in your Vultr account.
    ListDomains(ListArgs),
    /// List all DNS records for a domain.
    ListRecords(ListRecordsArgs),
    /// Create an example configuration file (mode 0600).
    InitConfig {
        /// Path for the config file (default: ~/.config/vultr-dns-updater/config.toml,
        /// under $XDG_CONFIG_HOME if set).
        #[arg(short, long, value_name = "PATH")]
        path: Option<PathBuf>,
    },
    /// Show the example configuration file content.
    ShowConfigExample,
    /// Manage systemd service for automatic DNS updates.
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
    /// Print the agent-facing reference: every command in brief (plain text, <1k tokens).
    Usage,
    /// Manage vultr-dns itself (update, etc.).
    #[command(name = "self")]
    SelfCmd(self_update::SelfCmd),
}

#[derive(Debug, Subcommand)]
enum ServiceAction {
    /// Install systemd service and timer for automatic DNS updates.
    ///
    /// Installs a system-level service that runs as your user.
    /// Will prompt for sudo password if needed.
    Install {
        /// Update interval in minutes.
        #[arg(short, long, value_name = "MINUTES", default_value_t = service::DEFAULT_INTERVAL,
              value_parser = clap::value_parser!(u32).range(1..))]
        interval: u32,
    },
    /// Uninstall systemd service and timer.
    Uninstall,
    /// Check the status of the systemd service and timer.
    Status,
}

#[derive(Debug, Args)]
struct ConfigArg {
    /// Path to configuration file.
    #[arg(short = 'c', long = "config", value_name = "PATH", value_parser = existing_path)]
    config: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct KeyArgs {
    #[command(flatten)]
    config: ConfigArg,

    /// Vultr API key (or set VULTR_API_KEY env var).
    #[arg(
        long,
        value_name = "KEY",
        env = "VULTR_API_KEY",
        hide_env_values = true
    )]
    api_key: Option<String>,
}

#[derive(Debug, Args)]
struct TargetArgs {
    /// Domain (with --subdomain: one target from flags instead of the config).
    #[arg(short, long, requires = "subdomain")]
    domain: Option<String>,

    /// Subdomain ("" for the apex; needs --domain).
    #[arg(short, long, requires = "domain")]
    subdomain: Option<String>,
}

#[derive(Debug, Args)]
struct UpdateArgs {
    #[command(flatten)]
    key: KeyArgs,

    #[command(flatten)]
    target: TargetArgs,

    /// TTL in seconds for the --domain/--subdomain target [default: 60].
    #[arg(short, long, value_name = "SECONDS", requires = "domain",
          value_parser = clap::value_parser!(u32).range(1..))]
    ttl: Option<u32>,

    /// Force update even if IP hasn't changed.
    #[arg(short, long)]
    force: bool,

    /// Show what would be done without making changes.
    #[arg(long)]
    dry_run: bool,

    /// Use a specific IPv4 instead of detecting automatically.
    #[arg(long, value_name = "IPV4", value_parser = ip::parse_ip_flag)]
    ip: Option<Ipv4Addr>,
}

#[derive(Debug, Args)]
struct StatusArgs {
    #[command(flatten)]
    key: KeyArgs,

    #[command(flatten)]
    target: TargetArgs,

    /// Output format.
    #[arg(long, value_enum, default_value_t)]
    format: Format,
}

#[derive(Debug, Args)]
struct ListArgs {
    #[command(flatten)]
    key: KeyArgs,

    /// Output format.
    #[arg(long, value_enum, default_value_t)]
    format: Format,
}

#[derive(Debug, Args)]
struct ListRecordsArgs {
    /// The domain whose records to list.
    domain: String,

    #[command(flatten)]
    list: ListArgs,
}

/// clap's check that `--config` exists (a usage error, exit 2, as in Python).
fn existing_path(s: &str) -> Result<PathBuf, String> {
    let p = PathBuf::from(s);
    if p.exists() {
        Ok(p)
    } else {
        Err(format!("Path '{s}' does not exist."))
    }
}

/// Every failure was already printed; just exit 1.
#[derive(Debug)]
struct AlreadyReported;

impl std::fmt::Display for AlreadyReported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("errors were reported")
    }
}

impl std::error::Error for AlreadyReported {}

fn main() -> ExitCode {
    output::init_color();
    let cli = Cli::parse();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to start the async runtime");
    match runtime.block_on(run(cli.command)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            if e.downcast_ref::<AlreadyReported>().is_none() {
                output::error(&format!("{e:#}"));
            }
            ExitCode::FAILURE
        }
    }
}

async fn run(command: Command) -> Result<()> {
    match command {
        Command::Update(args) => run_update(args).await,
        Command::Status(args) => run_status(args).await,
        Command::GetIp(args) => {
            let cfg = config::load_optional(args.config.as_deref())?;
            outln!("{}", detect_ip(cfg.as_ref()).await?);
            Ok(())
        }
        Command::ListDomains(args) => run_list_domains(args).await,
        Command::ListRecords(args) => run_list_records(args).await,
        Command::InitConfig { path } => {
            let path = config::init_config(path)?;
            output::success(&format!("Created example config at: {}", path.display()));
            outln!();
            outln!(
                "{}",
                colored::Colorize::dimmed("Edit the file to add your Vultr API key and targets.")
            );
            Ok(())
        }
        Command::ShowConfigExample => {
            outln!("{}", config::EXAMPLE_CONFIG);
            Ok(())
        }
        Command::Service { action } => match action {
            ServiceAction::Install { interval } => service::install(interval),
            ServiceAction::Uninstall => service::uninstall(),
            ServiceAction::Status => service::status(),
        },
        Command::Usage => {
            usage::run();
            Ok(())
        }
        Command::SelfCmd(cmd) => self_update::run(cmd).await,
    }
}

async fn detect_ip(cfg: Option<&Config>) -> Result<Ipv4Addr> {
    ip::detect(&Config::ip_check_urls(cfg)).await
}

/// Targets and config for `update` / `status`: with `-d`/`-s` one target
/// from the flags (the config, if any, still gives the key and the IP
/// services); otherwise the config is required and gives the targets.
fn resolve_targets(
    config_path: Option<&Path>,
    target: &TargetArgs,
    ttl: Option<u32>,
) -> Result<(Vec<Target>, Option<Config>)> {
    match (&target.domain, &target.subdomain) {
        (Some(domain), Some(subdomain)) => {
            if domain.trim().is_empty() {
                bail!("--domain must not be empty");
            }
            let cfg = config::load_optional(config_path)?;
            let t = Target {
                domain: domain.clone(),
                subdomain: subdomain.clone(),
                ttl: ttl.unwrap_or(DEFAULT_TTL),
            };
            Ok((vec![t], cfg))
        }
        _ => {
            let cfg = config::load_required(config_path)?;
            Ok((cfg.targets.clone(), Some(cfg)))
        }
    }
}

async fn run_update(args: UpdateArgs) -> Result<()> {
    let (targets, cfg) =
        resolve_targets(args.key.config.config.as_deref(), &args.target, args.ttl)?;
    let key = config::resolve_api_key(args.key.api_key.as_deref(), cfg.as_ref())?;
    if targets.is_empty() {
        bail!("No update targets configured. Add targets to config or use --domain/--subdomain");
    }

    let ip = match args.ip {
        Some(ip) => {
            output::info(&format!("Using custom IP: {ip}"));
            ip
        }
        None => {
            let ip = detect_ip(cfg.as_ref()).await?;
            output::info(&format!("Current public IP: {ip}"));
            ip
        }
    };

    if args.dry_run {
        outln!();
        outln!(
            "{}",
            colored::Colorize::yellow(colored::Colorize::bold(
                "DRY RUN MODE - No changes will be made"
            ))
        );
        outln!();
    }

    let client = vultr::Client::new(key)?;
    let failures = sync::update(&client, &targets, ip, args.force, args.dry_run).await;
    if failures > 0 {
        return Err(AlreadyReported.into());
    }
    Ok(())
}

async fn run_status(args: StatusArgs) -> Result<()> {
    let (targets, cfg) = resolve_targets(args.key.config.config.as_deref(), &args.target, None)?;
    let key = config::resolve_api_key(args.key.api_key.as_deref(), cfg.as_ref())?;
    if targets.is_empty() {
        bail!("No targets configured");
    }
    let ip = detect_ip(cfg.as_ref()).await?;
    let client = vultr::Client::new(key)?;
    let status = sync::status(&client, &targets, ip).await;

    match args.format {
        Format::Human => sync::print_status(&status),
        Format::Json => output::print_json(&status)?,
    }
    for t in &status.targets {
        if let Some(e) = &t.error {
            output::error(&format!("{}: {e}", t.fqdn));
        }
    }
    if status.failures() > 0 {
        return Err(AlreadyReported.into());
    }
    Ok(())
}

/// The key for the list commands: flag/env, else the config (if one is found).
fn list_key(args: &KeyArgs) -> Result<config::Secret> {
    let have_flag = args.api_key.as_deref().is_some_and(|k| !k.is_empty());
    let cfg = if have_flag && args.config.config.is_none() {
        None
    } else {
        config::load_optional(args.config.config.as_deref())?
    };
    config::resolve_api_key(args.api_key.as_deref(), cfg.as_ref())
}

#[derive(Tabled)]
struct DomainRow<'a> {
    #[tabled(rename = "Domain")]
    domain: &'a str,
    #[tabled(rename = "Created")]
    created: &'a str,
}

async fn run_list_domains(args: ListArgs) -> Result<()> {
    let client = vultr::Client::new(list_key(&args.key)?)?;
    let domains = client.list_domains().await?;
    match args.format {
        Format::Json => output::print_json(&domains),
        Format::Human if domains.is_empty() => {
            output::info("No domains found in your Vultr account.");
            Ok(())
        }
        Format::Human => {
            let rows = domains.iter().map(|d| DomainRow {
                domain: &d.domain,
                created: &d.date_created,
            });
            outln!("{}", output::table(rows));
            Ok(())
        }
    }
}

#[derive(Tabled, Serialize)]
struct RecordRow<'a> {
    #[tabled(rename = "ID")]
    id: &'a str,
    #[tabled(rename = "Type")]
    kind: &'a str,
    #[tabled(rename = "Name")]
    name: &'a str,
    #[tabled(rename = "Data")]
    data: &'a str,
    #[tabled(rename = "TTL")]
    ttl: u32,
    #[tabled(rename = "Priority")]
    priority: i32,
}

async fn run_list_records(args: ListRecordsArgs) -> Result<()> {
    let client = vultr::Client::new(list_key(&args.list.key)?)?;
    let records = client.list_records(&args.domain).await?;
    match args.list.format {
        Format::Json => output::print_json(&records),
        Format::Human if records.is_empty() => {
            output::info(&format!("No records found for domain: {}", args.domain));
            Ok(())
        }
        Format::Human => {
            let rows = records.iter().map(|r| RecordRow {
                id: &r.id,
                kind: &r.kind,
                name: if r.name.is_empty() { "@" } else { &r.name },
                data: &r.data,
                ttl: r.ttl,
                priority: r.priority,
            });
            outln!("{}", output::table(rows));
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_is_well_formed() {
        Cli::command().debug_assert();
    }

    #[test]
    fn domain_and_subdomain_require_each_other() {
        let err = |args: &[&str]| Cli::try_parse_from(args).unwrap_err().kind();
        use clap::error::ErrorKind::MissingRequiredArgument as Missing;
        assert_eq!(err(&["vultr-dns", "update", "-d", "a.com"]), Missing);
        assert_eq!(err(&["vultr-dns", "update", "-s", "home"]), Missing);
        assert_eq!(err(&["vultr-dns", "update", "-t", "300"]), Missing);
        assert_eq!(err(&["vultr-dns", "status", "-d", "a.com"]), Missing);
        assert!(Cli::try_parse_from(["vultr-dns", "update", "-d", "a.com", "-s", ""]).is_ok());
    }

    #[test]
    fn bad_flag_values_are_usage_errors() {
        for args in [
            &["vultr-dns", "update", "--ip", "::1"][..],
            &["vultr-dns", "update", "-d", "a", "-s", "b", "-t", "0"],
            &["vultr-dns", "service", "install", "-i", "0"],
            &["vultr-dns", "list-domains", "--format", "yaml"],
        ] {
            assert!(Cli::try_parse_from(args).is_err(), "{args:?}");
        }
    }
}
