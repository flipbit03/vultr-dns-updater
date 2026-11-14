//! The Vultr DNS API: list domains and records (every page), create and
//! patch A records.

use std::collections::HashSet;
use std::net::Ipv4Addr;

use anyhow::{Context, Result, bail};
use reqwest::{Method, Url};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::config::Secret;
use crate::output;

const DEFAULT_BASE_URL: &str = "https://api.vultr.com/v2";

/// Overrides the API base URL. For the tests; not in `--help` or `usage`.
pub const BASE_URL_ENV: &str = "VULTR_DNS_UPDATER_API_URL";

/// Vultr's maximum page size.
const PER_PAGE: &str = "500";

/// A runaway cursor loop stops here.
const MAX_PAGES: usize = 1000;

/// The longest body excerpt an error message carries.
const ERROR_BODY_MAX: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Domain {
    pub domain: String,
    #[serde(default)]
    pub date_created: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    /// Relative to the domain; the apex is `""`.
    pub name: String,
    pub data: String,
    /// `-1` on NS records.
    #[serde(default)]
    pub priority: i32,
    pub ttl: u32,
}

pub struct Client {
    http: reqwest::Client,
    base: Url,
    key: Secret,
}

impl Client {
    pub fn new(key: Secret) -> Result<Self> {
        let base = std::env::var(BASE_URL_ENV)
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        Self::with_base(key, &base)
    }

    pub(crate) fn with_base(key: Secret, base: &str) -> Result<Self> {
        let base = Url::parse(base)
            .ok()
            .filter(|u| matches!(u.scheme(), "http" | "https") && !u.cannot_be_a_base())
            .with_context(|| format!("invalid Vultr API URL {base}"))?;
        Ok(Self {
            http: crate::http::api_client(),
            base,
            key,
        })
    }

    /// The base URL plus path segments, each percent-encoded.
    fn url(&self, segments: &[&str]) -> Url {
        let mut url = self.base.clone();
        url.path_segments_mut()
            .expect("Client::new only accepts http(s) base URLs")
            .pop_if_empty()
            .extend(segments);
        url
    }

    async fn send(
        &self,
        method: Method,
        url: Url,
        body: Option<serde_json::Value>,
    ) -> Result<String> {
        let mut req = self
            .http
            .request(method, url)
            .bearer_auth(self.key.expose());
        if let Some(body) = body {
            req = req.json(&body);
        }
        let resp = req.send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        if status.as_u16() >= 400 {
            bail!(api_error(status.as_u16(), &text));
        }
        Ok(text)
    }

    /// Every item under `key` across all pages, following `meta.links.next`.
    async fn list_all<T: DeserializeOwned>(&self, segments: &[&str], key: &str) -> Result<Vec<T>> {
        let mut items = Vec::new();
        let mut cursor: Option<String> = None;
        let mut seen = HashSet::new();
        for _ in 0..MAX_PAGES {
            let mut url = self.url(segments);
            url.query_pairs_mut().append_pair("per_page", PER_PAGE);
            if let Some(c) = &cursor {
                url.query_pairs_mut().append_pair("cursor", c);
            }
            let text = self.send(Method::GET, url.clone(), None).await?;
            let page: Page<T> = page_from(&text, key)
                .with_context(|| format!("unexpected response from Vultr ({url})"))?;
            items.extend(page.items);
            match page.next {
                None => return Ok(items),
                Some(next) if seen.insert(next.clone()) => cursor = Some(next),
                // Stopping here would return a partial list, and a target
                // missing from it would be created again.
                Some(next) => bail!("Vultr returned the page cursor {next:?} twice ({url})"),
            }
        }
        bail!("Vultr kept returning more pages after {MAX_PAGES}; giving up")
    }

    pub async fn list_domains(&self) -> Result<Vec<Domain>> {
        self.list_all(&["domains"], "domains").await
    }

    pub async fn list_records(&self, domain: &str) -> Result<Vec<Record>> {
        self.list_all(&["domains", domain, "records"], "records")
            .await
    }

    /// Creates the A record and returns it as Vultr stored it (or as sent,
    /// if the answer can't be read).
    pub async fn create_record(
        &self,
        domain: &str,
        name: &str,
        ip: Ipv4Addr,
        ttl: u32,
    ) -> Result<Record> {
        let body = serde_json::json!({
            "name": name,
            "type": "A",
            "data": ip.to_string(),
            "ttl": ttl,
            "priority": 0,
        });
        let url = self.url(&["domains", domain, "records"]);
        let text = self.send(Method::POST, url, Some(body)).await?;
        #[derive(Deserialize)]
        struct Created {
            record: Record,
        }
        Ok(serde_json::from_str::<Created>(&text)
            .map(|c| c.record)
            .unwrap_or_else(|_| Record {
                id: String::new(),
                kind: "A".into(),
                name: name.into(),
                data: ip.to_string(),
                priority: 0,
                ttl,
            }))
    }

    pub async fn update_record(
        &self,
        domain: &str,
        id: &str,
        ip: Ipv4Addr,
        ttl: u32,
    ) -> Result<()> {
        let body = serde_json::json!({ "data": ip.to_string(), "ttl": ttl });
        let url = self.url(&["domains", domain, "records", id]);
        self.send(Method::PATCH, url, Some(body)).await?;
        Ok(())
    }
}

struct Page<T> {
    items: Vec<T>,
    next: Option<String>,
}

fn page_from<T: DeserializeOwned>(text: &str, key: &str) -> Result<Page<T>> {
    let mut value: serde_json::Value = serde_json::from_str(text).context("not JSON")?;
    let list = value
        .get_mut(key)
        .map(serde_json::Value::take)
        .with_context(|| format!("no \"{key}\" field"))?;
    let items = serde_json::from_value(list)?;
    let next = value
        .pointer("/meta/links/next")
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    Ok(Page { items, next })
}

/// `Vultr API error (<status>): <error field, or the body>`.
fn api_error(status: u16, body: &str) -> String {
    let field = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
        .filter(|e| !e.trim().is_empty());
    let message = match field {
        Some(e) => output::excerpt(&e, ERROR_BODY_MAX),
        None if body.trim().is_empty() => "(empty response)".to_string(),
        None => output::excerpt(body, ERROR_BODY_MAX),
    };
    let hint = if status == 401 {
        " (check api_key)"
    } else {
        ""
    };
    format!("Vultr API error ({status}): {message}{hint}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_error_uses_error_field() {
        assert_eq!(
            api_error(401, r#"{"error":"Invalid API token.","status":401}"#),
            "Vultr API error (401): Invalid API token. (check api_key)"
        );
        assert_eq!(api_error(500, "boom"), "Vultr API error (500): boom");
        assert_eq!(
            api_error(502, ""),
            "Vultr API error (502): (empty response)"
        );
        let long = "x".repeat(2000);
        assert!(api_error(500, &long).len() < 600);
    }

    #[test]
    fn record_parses_ns_priority_and_extra_fields() {
        let page: Page<Record> = page_from(
            r#"{"records":[{"id":"a","type":"NS","name":"","data":"ns1.vultr.com","priority":-1,"ttl":300,"extra":1}],
                "meta":{"total":1,"links":{"next":"","prev":""}}}"#,
            "records",
        )
        .unwrap();
        assert_eq!(page.items[0].priority, -1);
        assert_eq!(page.items[0].kind, "NS");
        assert_eq!(page.next, None);
    }

    #[test]
    fn page_next_cursor() {
        let page: Page<Domain> = page_from(
            r#"{"domains":[{"domain":"a.com","date_created":"2020"}],"meta":{"links":{"next":"abc"}}}"#,
            "domains",
        )
        .unwrap();
        assert_eq!(page.next.as_deref(), Some("abc"));
        assert!(page_from::<Domain>("{}", "domains").is_err());
    }

    #[test]
    fn bad_base_url_is_an_error() {
        for bad in ["mailto:x@y", "not a url", "ftp://x.io", ""] {
            let err = Client::with_base(Secret::new("k"), bad).err().unwrap();
            assert!(
                err.to_string().starts_with("invalid Vultr API URL"),
                "{bad}"
            );
        }
        assert!(Client::with_base(Secret::new("k"), "http://127.0.0.1:9/v2").is_ok());
    }

    #[test]
    fn domain_segment_is_encoded() {
        let c = Client {
            http: reqwest::Client::new(),
            base: Url::parse("https://api.vultr.com/v2").unwrap(),
            key: Secret::new("k"),
        };
        assert_eq!(
            c.url(&["domains", "a b/c.com", "records"]).as_str(),
            "https://api.vultr.com/v2/domains/a%20b%2Fc.com/records"
        );
        let c = Client {
            base: Url::parse("http://127.0.0.1:9/").unwrap(),
            ..c
        };
        assert_eq!(c.url(&["domains"]).as_str(), "http://127.0.0.1:9/domains");
    }
}
