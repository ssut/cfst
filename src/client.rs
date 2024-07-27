use anyhow::Result;
use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::bandwidth_test_client::BandwidthTestClient;
use crate::calculate_measurements;

#[derive(Debug, Deserialize)]
pub struct CfCdnCgiTrace {
    pub ip: String,
    pub loc: String,
    pub colo: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfMeta {
    pub hostname: String,
    pub client_ip: String,
    pub http_protocol: String,
    pub asn: u32,
    pub as_organization: String,
    pub colo: String,
    pub country: String,
    pub city: String,
    pub region: String,
    pub postal_code: String,
    pub latitude: String,
    pub longitude: String,
}

pub struct RequestTiming {
    pub start: Instant,
    pub ttfb: Instant,
    pub end: Option<Instant>,
    pub server_timing: f64,
}

#[derive(Debug, Deserialize)]
struct Location {
    pub iata: String,
    pub city: String,
}

pub struct SpeedTestClient {
    http_client: Client,
    bandwidth_test_client: BandwidthTestClient,
}

impl SpeedTestClient {
    pub fn new() -> Self {
        let http_client = Client::builder()
            .tcp_keepalive(Some(Duration::from_secs(60)))
            .tcp_nodelay(true)
            .build()
            .unwrap();
        let bandwidth_test_client = BandwidthTestClient::new();

        SpeedTestClient {
            http_client,
            bandwidth_test_client,
        }
    }

    pub async fn get(&self, url: &str) -> Result<String> {
        let res = self.http_client.get(url).send().await?.text().await?;
        Ok(res)
    }

    pub async fn fetch_server_location_data(&self) -> Result<HashMap<String, String>> {
        let url = "https://speed.cloudflare.com/locations";
        let res = self.get(url).await?;
        let json: Vec<Location> = serde_json::from_str(&res)?;
        let mut data = HashMap::new();

        for item in json {
            data.insert(item.iata, item.city);
        }

        Ok(data)
    }

    pub async fn fetch_cf_cdn_cgi_trace(&self) -> Result<CfCdnCgiTrace> {
        let url = "https://speed.cloudflare.com/cdn-cgi/trace";
        let res = self.get(url).await?;
        let lines: Vec<&str> = res.split('\n').collect();
        let mut trace = CfCdnCgiTrace {
            ip: "".to_string(),
            loc: "".to_string(),
            colo: "".to_string(),
        };

        for line in lines {
            let parts: Vec<&str> = line.split('=').collect();
            if parts.len() == 2 {
                match parts[0] {
                    "ip" => trace.ip = parts[1].to_string(),
                    "loc" => trace.loc = parts[1].to_string(),
                    "colo" => trace.colo = parts[1].to_string(),
                    _ => {}
                }
            }
        }

        Ok(trace)
    }

    pub async fn fetch_meta(&self) -> Result<CfMeta> {
        let url = "https://speed.cloudflare.com/meta";
        let res = self.get(url).await?;
        let meta: CfMeta = serde_json::from_str(&res)?;
        Ok(meta)
    }

    /**
     * Request -> request only (do not read the response body)
     */
    pub async fn request(&self, url: &str) -> Result<RequestTiming> {
        let start = Instant::now();
        let res = self.http_client.get(url).send().await?;
        let ttfb = Instant::now();
        let server_timing = res
            .headers()
            .get("server-timing")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split(";dur=").nth(1))
            .and_then(|dur| dur.parse::<f64>().ok())
            .unwrap_or(0.0);

        Ok(RequestTiming {
            start,
            ttfb,
            end: None,
            server_timing,
        })
    }

    pub async fn download(&self, bytes: usize) -> Result<RequestTiming> {
        let url = format!("https://speed.cloudflare.com/__down?bytes={}", bytes);
        self.request(&url).await
    }

    pub async fn upload(&self, bytes: usize) -> Result<RequestTiming> {
        let url = "https://speed.cloudflare.com/__up";
        let data = "0".repeat(bytes);
        let start = Instant::now();
        let res = self.http_client.post(url).body(data).send().await?;
        let end = Instant::now();
        let server_timing = res
            .headers()
            .get("server-timing")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split(";dur=").nth(1))
            .and_then(|dur| dur.parse::<f64>().ok())
            .unwrap_or(0.0);

        Ok(RequestTiming {
            start,
            ttfb: end, // upload ttfb is the end in this context
            end: Some(end),
            server_timing,
        })
    }

    pub async fn measure_latency(&self) -> Result<Vec<f64>> {
        let bar = ProgressBar::new_spinner();
        bar.set_style(ProgressStyle::with_template("{msg} {spinner:.green}").unwrap());
        bar.set_message(format!(
            "{:>20}: --.--ms (mean: --.--ms, median: --.--ms, p95: --.--ms)",
            "Latency".bold()
        ));

        let mut measurements = Vec::new();
        let started_at = Instant::now();

        for _ in 0..100 {
            let timing = self.download(0).await?;
            measurements.push(
                (timing.ttfb - timing.start).as_micros() as f64 - timing.server_timing * 1000.0,
            );

            let result = calculate_measurements(&measurements);
            bar.set_message(format!(
                "{:>20}: {} ms (jitter: {}ms, mean: {}ms, median: {}ms, low: {}ms, high: {}ms)",
                "Latency".bold(),
                format!("{:.2}", result.last / 1000.0).magenta(),
                format!("{:.2}", super::stats::jitter(&measurements) / 1000.0).magenta(),
                format!("{:.2}", result.mean / 1000.0).magenta(),
                format!("{:.2}", result.median / 1000.0).magenta(),
                format!("{:.2}", result.low / 1000.0).magenta(),
                format!("{:.2}", result.high / 1000.0).magenta(),
            ));
            bar.tick();

            if measurements.len() >= 30 && started_at.elapsed() > Duration::from_secs(5) {
                break;
            }
        }

        bar.finish_and_clear();

        let result = calculate_measurements(&measurements);
        println!(
            "{:>20}: {} ms (jitter: {}ms, mean: {}ms, median: {}ms, low: {}ms, high: {}ms)",
            "Latency".bold(),
            format!("{:.2}", result.last / 1000.0).magenta(),
            format!("{:.2}", super::stats::jitter(&measurements) / 1000.0).magenta(),
            format!("{:.2}", result.mean / 1000.0).magenta(),
            format!("{:.2}", result.median / 1000.0).magenta(),
            format!("{:.2}", result.low / 1000.0).magenta(),
            format!("{:.2}", result.high / 1000.0).magenta(),
        );

        Ok(measurements)
    }

    pub async fn measure_download(&self) -> Result<Vec<f64>> {
        self.bandwidth_test_client.measure_download().await
    }

    pub async fn measure_upload(&self) -> Result<Vec<f64>> {
        self.bandwidth_test_client.measure_upload().await
    }
}
