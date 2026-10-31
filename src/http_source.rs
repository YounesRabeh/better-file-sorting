// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Younidev. See LICENSE for license terms.

//! Discovers direct ZIP links in HTML and centralizes HTTP request restrictions.
//! URL validation, redirect checks, origin boundaries, request caps and spacing are
//! shared by page discovery and payload transfers. HTML parsing resolves relative links
//! and extracts visible titles without crawling pages or executing JavaScript.

/// Maximum redirect hops; a fixed protocol safeguard shared by acquisition commands.
const MAX_REDIRECTS: usize = 5;
/// Upper bound on cancellation latency during pacing waits.
const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(25);

use crate::acquisition::*;
use anyhow::{Result, ensure};
use std::{
    collections::BTreeMap,
    io::Read,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use url::Url;

pub fn safe_url(value: &str) -> Result<Url> {
    let mut url = Url::parse(value).map_err(|_| anyhow::anyhow!("invalid source URL"))?;
    ensure!(
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none(),
        "only HTTP(S) URLs without credentials or queries are supported"
    );
    url.set_fragment(None);
    Ok(url)
}
pub fn wait_until(deadline: Instant, cancelled: &AtomicBool) -> Result<()> {
    while Instant::now() < deadline {
        ensure!(!cancelled.load(Ordering::Relaxed), "interrupted");
        std::thread::sleep(
            deadline
                .saturating_duration_since(Instant::now())
                .min(CANCEL_POLL_INTERVAL),
        );
    }
    ensure!(!cancelled.load(Ordering::Relaxed), "interrupted");
    Ok(())
}
struct Gate {
    count: usize,
    next: Instant,
    blocked: std::collections::BTreeSet<String>,
}
pub struct HttpSource {
    agent: ureq::Agent,
    origin: Url,
    same_origin: bool,
    interval: Duration,
    timeout: Duration,
    max_requests: Option<usize>,
    gate: Mutex<Gate>,
}
impl HttpSource {
    pub fn new(source: &SourceConfig, config: &DownloadConfig) -> Result<Self> {
        config.validate()?;
        Ok(Self {
            agent: ureq::Agent::config_builder()
                .max_redirects(0)
                .http_status_as_error(false)
                .timeout_global(Some(Duration::from_secs(config.timeout_seconds)))
                .build()
                .into(),
            origin: safe_url(&source.page)?,
            same_origin: source.same_origin,
            interval: Duration::from_millis(config.min_request_interval_ms),
            timeout: Duration::from_secs(config.timeout_seconds),
            max_requests: config.max_requests,
            gate: Mutex::new(Gate {
                count: 0,
                next: Instant::now(),
                blocked: Default::default(),
            }),
        })
    }
    pub fn get(
        &self,
        value: &str,
        cancelled: &AtomicBool,
    ) -> Result<(ureq::http::Response<ureq::Body>, Url)> {
        let deadline = Instant::now()
            .checked_add(self.timeout)
            .ok_or_else(|| anyhow::anyhow!("timeout too large"))?;
        let mut url = safe_url(value)?;
        for hop in 0..=MAX_REDIRECTS {
            ensure!(
                !self.same_origin || url.origin() == self.origin.origin(),
                "cross-origin request refused"
            );
            {
                let mut gate = self.gate.lock().unwrap();
                ensure!(
                    !gate.blocked.contains(&url.origin().ascii_serialization()),
                    "origin rate limited"
                );
                ensure!(
                    self.max_requests.is_none_or(|n| gate.count < n),
                    "request limit reached"
                );
                wait_until(gate.next.min(deadline), cancelled)?;
                ensure!(Instant::now() < deadline, "request pacing timeout");
                gate.count += 1;
                gate.next = Instant::now()
                    .checked_add(self.interval)
                    .ok_or_else(|| anyhow::anyhow!("request interval too large"))?;
            }
            let response = self
                .agent
                .get(url.as_str())
                .header("Accept-Encoding", "identity")
                .call()
                .map_err(|_| anyhow::anyhow!("HTTP request failed"))?;
            if response.status().as_u16() == 429 {
                self.gate
                    .lock()
                    .unwrap()
                    .blocked
                    .insert(url.origin().ascii_serialization());
                let retry = response
                    .headers()
                    .get("retry-after")
                    .and_then(|h| h.to_str().ok())
                    .filter(|s| s.bytes().all(|b| b.is_ascii_digit()))
                    .unwrap_or("unspecified");
                anyhow::bail!("HTTP 429; Retry-After: {retry}");
            }
            if response.status().is_redirection() {
                ensure!(hop < MAX_REDIRECTS, "redirect limit reached");
                let location = response
                    .headers()
                    .get("location")
                    .and_then(|v| v.to_str().ok())
                    .ok_or_else(|| anyhow::anyhow!("missing redirect location"))?;
                let next = url
                    .join(location)
                    .map_err(|_| anyhow::anyhow!("invalid redirect"))?;
                ensure!(
                    url.scheme() != "https" || next.scheme() == "https",
                    "HTTPS downgrade refused"
                );
                url = safe_url(next.as_str())?;
                continue;
            }
            ensure!(
                response.status().is_success(),
                "HTTP status {}",
                response.status().as_u16()
            );
            ensure!(
                response
                    .headers()
                    .get("content-encoding")
                    .is_none_or(|v| v == "identity"),
                "encoded response refused"
            );
            return Ok((response, url));
        }
        unreachable!()
    }
}
pub fn parse_links(config: &SourceConfig, final_url: &str, html: &str) -> Result<Inventory> {
    let origin = safe_url(&config.page)?;
    let page = safe_url(final_url)?;
    let document = scraper::Html::parse_document(html);
    let base_selector = scraper::Selector::parse("base[href]").unwrap();
    let base = document
        .select(&base_selector)
        .find_map(|e| {
            page.join(e.value().attr("href")?)
                .ok()
                .and_then(|u| safe_url(u.as_str()).ok())
                .filter(|u| !config.same_origin || u.origin() == origin.origin())
        })
        .unwrap_or(page);
    let selector = scraper::Selector::parse("a[href]").unwrap();
    let mut candidates = BTreeMap::new();
    let date = chrono::Utc::now().to_rfc3339();
    for element in document.select(&selector) {
        let Ok(url) = base.join(element.value().attr("href").unwrap()) else {
            continue;
        };
        let Ok(url) = safe_url(url.as_str()) else {
            continue;
        };
        if config.same_origin && url.origin() != origin.origin() {
            continue;
        }
        if !url.path().to_lowercase().ends_with(".zip") {
            continue;
        }
        let name = url
            .path_segments()
            .and_then(|mut p| p.next_back())
            .unwrap_or("download.zip")
            .to_string();
        let title = element
            .text()
            .collect::<Vec<_>>()
            .join(" ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let source_id = crate::metadata::digest(url.as_str().as_bytes());
        candidates.entry(source_id.clone()).or_insert(Candidate {
            source_id,
            url: url.to_string(),
            title: if title.is_empty() {
                name.clone()
            } else {
                title
            },
            name,
            discovered_at: date.clone(),
        });
    }
    Ok(Inventory {
        version: 1,
        candidates: candidates.into_values().collect(),
    })
}
pub fn discover(
    config: &SourceConfig,
    download: &DownloadConfig,
    cancelled: &AtomicBool,
) -> Result<Inventory> {
    let http = HttpSource::new(config, download)?;
    let (mut response, url) = http.get(&config.page, cancelled)?;
    ensure!(
        response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|s| s
                .split(';')
                .next()
                .unwrap()
                .trim()
                .eq_ignore_ascii_case("text/html")),
        "expected HTML page"
    );
    let limit = parse_bytes(&config.max_page_size)?;
    let mut bytes = vec![];
    let mut reader = response.body_mut().as_reader();
    let mut buffer = [0; 8192];
    loop {
        ensure!(!cancelled.load(Ordering::Relaxed), "interrupted");
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        ensure!(
            (bytes.len() as u64) + n as u64 <= limit,
            "page size limit reached"
        );
        bytes.extend_from_slice(&buffer[..n]);
    }
    parse_links(config, url.as_str(), std::str::from_utf8(&bytes)?)
}
