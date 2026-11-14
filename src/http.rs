//! The HTTP clients: one builder, three timeouts.
//!
//! TLS is rustls on `ring` with Mozilla's roots compiled in (reqwest's
//! `rustls-tls`), so nothing is read from the OS at runtime.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};

/// Sent on every request.
pub const USER_AGENT: &str = concat!("vultr-dns-updater/", env!("CARGO_PKG_VERSION"));

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Vultr and the GitHub API.
const API_TIMEOUT: Duration = Duration::from_secs(30);

/// Each public-IP service (the Python tool's 10 s).
const IP_CHECK_TIMEOUT: Duration = Duration::from_secs(10);

/// A release binary, over a slow link.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(5 * 60);

fn builder(timeout: Duration) -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(CONNECT_TIMEOUT.min(timeout))
        .timeout(timeout)
}

/// For Vultr and the GitHub API.
pub fn api_client() -> reqwest::Client {
    builder(API_TIMEOUT)
        .build()
        .expect("failed to build HTTP client")
}

/// For release downloads.
pub fn download_client() -> reqwest::Client {
    builder(DOWNLOAD_TIMEOUT)
        .build()
        .expect("failed to build HTTP client")
}

/// For the public-IP services: IPv4 only, so a dual-stack service can't
/// answer with the IPv6 address. Both halves on purpose: the v4-only resolver
/// never hands out a v6 address, and the v4 local address can't reach one.
pub fn ip_client() -> reqwest::Client {
    builder(IP_CHECK_TIMEOUT)
        .local_address(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
        .dns_resolver(Arc::new(Ipv4Only))
        .build()
        .expect("failed to build HTTP client")
}

/// The system resolver, keeping only the IPv4 answers.
struct Ipv4Only;

impl Resolve for Ipv4Only {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(async move {
            let host = name.as_str().to_string();
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await?
                .filter(SocketAddr::is_ipv4)
                .collect();
            if addrs.is_empty() {
                return Err(format!("{host} has no IPv4 address").into());
            }
            let addrs: Addrs = Box::new(addrs.into_iter());
            Ok(addrs)
        })
    }
}
