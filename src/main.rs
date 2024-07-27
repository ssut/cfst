mod bandwidth_test_client;
mod client;
mod stats;
mod utils;
use colored::*;

use anyhow::Result;
use client::*;
use utils::*;

#[tokio::main]
async fn main() -> Result<()> {
    let client = SpeedTestClient::new();

    let (server_location_data, trace, meta) = tokio::try_join!(
        client.fetch_server_location_data(),
        client.fetch_cf_cdn_cgi_trace(),
        client.fetch_meta(),
    )?;

    let default_city = "Unknown".to_string();
    let city = server_location_data
        .get(&trace.colo)
        .unwrap_or(&default_city);

    println!(
        "{:>20}: {}",
        "Server location".bold(),
        format!("{} ({})", city, trace.colo).blue(),
    );
    println!(
        "{:>20}: {}",
        "Your IP".bold(),
        format!("{} ({}, {})", meta.client_ip, meta.city, meta.country).blue(),
    );
    println!(
        "{:>20}: {}",
        "ISP".bold(),
        format!("{} (AS{})", meta.as_organization, meta.asn).blue(),
    );

    client.measure_latency().await?;
    client.measure_download().await?;
    client.measure_upload().await?;

    Ok(())
}
