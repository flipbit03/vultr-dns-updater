//! Public IPv4 detection: ask each service in order, take the first answer
//! that is a public IPv4 address.

use std::net::Ipv4Addr;

use anyhow::{Result, bail};

use crate::output;

/// The house's public address, from the first service that gives a good one.
pub async fn detect(urls: &[String]) -> Result<Ipv4Addr> {
    let client = crate::http::ip_client();
    let mut errors = Vec::new();
    for url in urls {
        match ask(&client, url).await {
            Ok(ip) => return Ok(ip),
            Err(reason) => errors.push(format!("  - {url}: {reason}")),
        }
    }
    bail!("Failed to detect public IP. Errors:\n{}", errors.join("\n"))
}

async fn ask(client: &reqwest::Client, url: &str) -> Result<Ipv4Addr, String> {
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| output::chain(&e.without_url()))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    let body = resp
        .text()
        .await
        .map_err(|e| output::chain(&e.without_url()))?;
    parse_public(&body)
}

/// The trimmed body as a public IPv4 address, or why it isn't one.
pub fn parse_public(body: &str) -> Result<Ipv4Addr, String> {
    let text = body.trim();
    let ip: Ipv4Addr = text
        .parse()
        .map_err(|_| format!("Invalid IP format: {}", output::excerpt(text, 100)))?;
    if !is_public(ip) {
        return Err(format!("{ip} is not a public IPv4 address"));
    }
    Ok(ip)
}

/// False for what can't be the house's public address: private, loopback,
/// link-local, CGNAT, unspecified, broadcast, documentation, multicast and
/// reserved ranges. A captive portal shouldn't point the record at `10.x`.
pub fn is_public(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    let this_network = a == 0;
    let cgnat = a == 100 && (64..128).contains(&b);
    let reserved = a >= 240;
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
        || this_network
        || cgnat
        || reserved)
}

/// clap's parser for `--ip`: any IPv4 address (no public-range check, so
/// tests and odd setups can still use it).
pub fn parse_ip_flag(s: &str) -> Result<Ipv4Addr, String> {
    s.trim()
        .parse()
        .map_err(|_| format!("\"{s}\" is not an IPv4 address"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_whitespace() {
        assert_eq!(parse_public(" 8.8.4.4\n"), Ok(Ipv4Addr::new(8, 8, 4, 4)));
        assert_eq!(parse_public("45.77.1.2"), Ok(Ipv4Addr::new(45, 77, 1, 2)));
    }

    #[test]
    fn rejects_v6_and_garbage() {
        for body in ["2001:db8::1", "<html>hi</html>", "", "1.2.3", "1.2.3.256"] {
            let err = parse_public(body).unwrap_err();
            assert!(err.starts_with("Invalid IP format"), "{body}: {err}");
        }
    }

    #[test]
    fn rejects_non_public_ranges() {
        for ip in [
            "10.0.0.1",
            "172.16.5.4",
            "192.168.1.1",
            "127.0.0.1",
            "169.254.1.1",
            "100.64.0.1",
            "100.127.255.255",
            "0.0.0.0",
            "0.1.2.3",
            "255.255.255.255",
            "192.0.2.1",
            "198.51.100.1",
            "203.0.113.9",
            "224.0.0.1",
            "240.0.0.1",
        ] {
            let err = parse_public(ip).unwrap_err();
            assert!(err.contains("not a public IPv4"), "{ip}: {err}");
        }
        assert!(is_public(Ipv4Addr::new(100, 63, 255, 255)));
        assert!(is_public(Ipv4Addr::new(100, 128, 0, 0)));
    }

    #[test]
    fn ip_flag_takes_any_v4() {
        assert_eq!(parse_ip_flag("10.0.0.1"), Ok(Ipv4Addr::new(10, 0, 0, 1)));
        assert!(parse_ip_flag("::1").is_err());
        assert!(parse_ip_flag("home").is_err());
    }
}
