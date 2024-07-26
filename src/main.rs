mod network;
mod stats;
mod utils;

use anyhow::Result;
use network::*;
use utils::*;

#[tokio::main]
async fn main() -> Result<()> {
    let client = SpeedTestClient::new();

    let (server_location_data, trace) = tokio::try_join!(
        client.fetch_server_location_data(),
        client.fetch_cf_cdn_cgi_trace()
    )?;

    let default_city = "Unknown".to_string();
    let city = server_location_data
        .get(&trace.colo)
        .unwrap_or(&default_city);

    log_info("Server location", &format!("{} ({})", city, trace.colo));
    log_info("Your IP", &format!("{} ({})", trace.ip, trace.loc));

    let latency_tests = client.measure_latency().await?;
    log_latency_result(latency_tests);

    let download_tests = client.measure_stabilized_download().await?;
    log_download_speed(&download_tests);

    let upload_tests = client.measure_stabilized_upload().await?;
    log_upload_speed(&upload_tests);

    Ok(())
}
