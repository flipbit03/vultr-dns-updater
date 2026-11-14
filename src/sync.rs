//! What each target needs (`decide`, pure) and doing it: drives `update`
//! and `status`, so "Needs update" means exactly what `update` would do.

use std::collections::HashMap;
use std::net::Ipv4Addr;

use colored::Colorize;
use serde::Serialize;
use tabled::Tabled;

use crate::config::Target;
use crate::output;
use crate::vultr::{Client, Record};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    UpToDate,
    Create,
    Update { old_ip: String, old_ttl: u32 },
}

/// What a target needs, given its existing A record (if any).
pub fn decide(existing: Option<&Record>, ip: Ipv4Addr, ttl: u32, force: bool) -> Action {
    match existing {
        None => Action::Create,
        Some(r) if !force && r.data == ip.to_string() && r.ttl == ttl => Action::UpToDate,
        Some(r) => Action::Update {
            old_ip: r.data.clone(),
            old_ttl: r.ttl,
        },
    }
}

/// The first A record named `name`, and how many there are.
pub fn find_record<'a>(records: &'a [Record], name: &str) -> (Option<&'a Record>, usize) {
    let mut matches = records.iter().filter(|r| r.kind == "A" && r.name == name);
    let first = matches.next();
    let count = first.map_or(0, |_| 1 + matches.count());
    (first, count)
}

/// `old -> new`, plus the TTL change when only the TTL differs.
fn describe_change(old_ip: &str, old_ttl: u32, ip: Ipv4Addr, ttl: u32) -> String {
    let ip = ip.to_string();
    if old_ip == ip && old_ttl != ttl {
        format!("{old_ip} -> {ip} (TTL: {old_ttl} -> {ttl})")
    } else {
        format!("{old_ip} -> {ip}")
    }
}

/// Each domain's records, listed once (all pages) however many targets use it.
struct Records<'a> {
    client: &'a Client,
    by_domain: HashMap<String, Result<Vec<Record>, String>>,
}

impl<'a> Records<'a> {
    fn new(client: &'a Client) -> Self {
        Self {
            client,
            by_domain: HashMap::new(),
        }
    }

    async fn get(&mut self, domain: &str) -> &Result<Vec<Record>, String> {
        if !self.by_domain.contains_key(domain) {
            let result = self
                .client
                .list_records(domain)
                .await
                .map_err(|e| format!("{e:#}"));
            self.by_domain.insert(domain.to_string(), result);
        }
        &self.by_domain[domain]
    }
}

fn duplicate_warning(count: usize, target: &Target, record: &Record) {
    if count > 1 {
        output::warning(&format!(
            "  {count} A records named \"{}\" in {}; updating {} only",
            target.subdomain, target.domain, record.id
        ));
    }
}

/// `vultr-dns update`: every target, even after one fails. Returns how many
/// failed (each already printed as an `Error:` line).
pub async fn update(
    client: &Client,
    targets: &[Target],
    ip: Ipv4Addr,
    force: bool,
    dry_run: bool,
) -> usize {
    let mut records = Records::new(client);
    let mut failures = 0;
    for target in targets {
        let fqdn = target.fqdn();
        println!();
        println!("{} {fqdn}", "Processing:".bold());

        let list = match records.get(&target.domain).await {
            Ok(list) => list,
            Err(e) => {
                output::error(e);
                failures += 1;
                continue;
            }
        };
        let (existing, count) = find_record(list, &target.subdomain);
        match existing {
            Some(r) => {
                println!("  Current: {} (TTL: {})", r.data, r.ttl);
                duplicate_warning(count, target, r);
            }
            None => println!("  Current: {}", "No record exists".dimmed()),
        }
        let existing_id = existing.map(|r| r.id.clone());

        let result = match decide(existing, ip, target.ttl, force) {
            Action::UpToDate => {
                output::info(&format!("  {fqdn} is already up to date"));
                Ok(())
            }
            Action::Update { old_ip, old_ttl } => {
                let change = describe_change(&old_ip, old_ttl, ip, target.ttl);
                if dry_run {
                    println!("  {} {change}", "Would update:".yellow());
                    Ok(())
                } else {
                    let id = existing_id.expect("an update has a record");
                    client
                        .update_record(&target.domain, &id, ip, target.ttl)
                        .await
                        .map(|()| output::success(&format!("  Updated {fqdn}: {change}")))
                }
            }
            Action::Create => {
                if dry_run {
                    println!("  {} {} A {ip}", "Would create:".yellow(), target.subdomain);
                    Ok(())
                } else {
                    client
                        .create_record(&target.domain, &target.subdomain, ip, target.ttl)
                        .await
                        .map(|()| output::success(&format!("  Created {fqdn} -> {ip}")))
                }
            }
        };
        if let Err(e) = result {
            output::error(&format!("{e:#}"));
            failures += 1;
        }
    }
    failures
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    UpToDate,
    NeedsUpdate,
    NotFound,
    Error,
}

impl Verdict {
    fn label(self) -> &'static str {
        match self {
            Verdict::UpToDate => "Up to date",
            Verdict::NeedsUpdate => "Needs update",
            Verdict::NotFound => "Not found",
            Verdict::Error => "Error",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct TargetStatus {
    pub fqdn: String,
    pub domain: String,
    pub subdomain: String,
    pub ttl: u32,
    pub status: Verdict,
    pub record: Option<Record>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Status {
    pub ip: Ipv4Addr,
    pub targets: Vec<TargetStatus>,
}

impl Status {
    pub fn failures(&self) -> usize {
        self.targets
            .iter()
            .filter(|t| t.status == Verdict::Error)
            .count()
    }
}

/// `vultr-dns status`: what `update` would do for each target, changing nothing.
pub async fn status(client: &Client, targets: &[Target], ip: Ipv4Addr) -> Status {
    let mut records = Records::new(client);
    let mut out = Vec::with_capacity(targets.len());
    for target in targets {
        let (verdict, record, error) = match records.get(&target.domain).await {
            Err(e) => (Verdict::Error, None, Some(e.clone())),
            Ok(list) => {
                let (existing, count) = find_record(list, &target.subdomain);
                if let Some(r) = existing {
                    duplicate_warning(count, target, r);
                }
                let verdict = match decide(existing, ip, target.ttl, false) {
                    Action::UpToDate => Verdict::UpToDate,
                    Action::Update { .. } => Verdict::NeedsUpdate,
                    Action::Create => Verdict::NotFound,
                };
                (verdict, existing.cloned(), None)
            }
        };
        out.push(TargetStatus {
            fqdn: target.fqdn(),
            domain: target.domain.clone(),
            subdomain: target.subdomain.clone(),
            ttl: target.ttl,
            status: verdict,
            record,
            error,
        });
    }
    Status { ip, targets: out }
}

#[derive(Tabled)]
struct StatusRow {
    #[tabled(rename = "FQDN")]
    fqdn: String,
    #[tabled(rename = "Current DNS")]
    current: String,
    #[tabled(rename = "Status")]
    status: String,
}

/// The human `status` output: the IP, then one row per target.
pub fn print_status(status: &Status) {
    println!("{} {}", "Current Public IP:".bold(), status.ip);
    println!();
    let rows = status.targets.iter().map(|t| StatusRow {
        fqdn: t.fqdn.clone(),
        current: t.record.as_ref().map_or_else(
            || "-".to_string(),
            |r| format!("{} (TTL: {})", r.data, r.ttl),
        ),
        status: t.status.label().to_string(),
    });
    println!("{}", output::table(rows));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(id: &str, kind: &str, name: &str, data: &str, ttl: u32) -> Record {
        Record {
            id: id.into(),
            kind: kind.into(),
            name: name.into(),
            data: data.into(),
            priority: 0,
            ttl,
        }
    }

    const IP: Ipv4Addr = Ipv4Addr::new(45, 77, 1, 2);

    #[test]
    fn decide_cases() {
        assert_eq!(decide(None, IP, 60, false), Action::Create);
        assert_eq!(decide(None, IP, 60, true), Action::Create);
        let same = rec("1", "A", "home", "45.77.1.2", 60);
        assert_eq!(decide(Some(&same), IP, 60, false), Action::UpToDate);
        assert_eq!(
            decide(Some(&same), IP, 60, true),
            Action::Update {
                old_ip: "45.77.1.2".into(),
                old_ttl: 60
            }
        );
        let ip_differs = rec("1", "A", "home", "1.1.1.1", 60);
        assert_eq!(
            decide(Some(&ip_differs), IP, 60, false),
            Action::Update {
                old_ip: "1.1.1.1".into(),
                old_ttl: 60
            }
        );
        let ttl_differs = rec("1", "A", "home", "45.77.1.2", 300);
        assert_eq!(
            decide(Some(&ttl_differs), IP, 60, false),
            Action::Update {
                old_ip: "45.77.1.2".into(),
                old_ttl: 300
            }
        );
        let both = rec("1", "A", "home", "1.1.1.1", 300);
        assert_eq!(
            decide(Some(&both), IP, 60, false),
            Action::Update {
                old_ip: "1.1.1.1".into(),
                old_ttl: 300
            }
        );
    }

    #[test]
    fn find_record_matches_type_and_exact_name() {
        let records = vec![
            rec("1", "AAAA", "home", "::1", 60),
            rec("2", "A", "Home", "1.1.1.1", 60),
            rec("3", "A", "home", "1.1.1.1", 60),
            rec("4", "A", "home", "2.2.2.2", 60),
            rec("5", "A", "", "3.3.3.3", 60),
        ];
        let (r, n) = find_record(&records, "home");
        assert_eq!((r.map(|r| r.id.as_str()), n), (Some("3"), 2));
        let (r, n) = find_record(&records, "");
        assert_eq!((r.map(|r| r.id.as_str()), n), (Some("5"), 1));
        assert_eq!(find_record(&records, "work"), (None, 0));
    }

    #[test]
    fn change_text() {
        assert_eq!(
            describe_change("1.1.1.1", 60, IP, 60),
            "1.1.1.1 -> 45.77.1.2"
        );
        assert_eq!(
            describe_change("1.1.1.1", 300, IP, 60),
            "1.1.1.1 -> 45.77.1.2"
        );
        assert_eq!(
            describe_change("45.77.1.2", 300, IP, 60),
            "45.77.1.2 -> 45.77.1.2 (TTL: 300 -> 60)"
        );
        assert_eq!(
            describe_change("45.77.1.2", 60, IP, 60),
            "45.77.1.2 -> 45.77.1.2"
        );
    }
}
