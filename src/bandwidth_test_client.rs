use anyhow::Result;
use bytes::Bytes;
use colored::*;
use futures::{Stream, StreamExt};

use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use std::{
    pin::Pin,
    task::{Context, Poll},
    time::{Duration, Instant},
};
use tokio::sync::mpsc::{Receiver, Sender};
use tokio_util::sync::CancellationToken;

pub struct BandwidthTestClient {
    http_client: Client,
}

impl BandwidthTestClient {
    pub fn new() -> Self {
        let http_client = Client::builder()
            .tcp_keepalive(Some(Duration::from_secs(60)))
            .tcp_nodelay(true)
            .build()
            .unwrap();
        BandwidthTestClient { http_client }
    }

    async fn producer(sizes: &[usize], tx_size: Sender<usize>, cancel_token: CancellationToken) {
        let start_time = Instant::now();
        let mut current_size_index = 0;

        while start_time.elapsed() < Duration::from_secs(20) {
            if cancel_token.is_cancelled() {
                break;
            }

            let size = sizes[current_size_index];
            tx_size.send(size).await.unwrap();

            let size_start_time = Instant::now();
            let elapsed_time = size_start_time.elapsed();

            if elapsed_time < Duration::from_secs(1) && current_size_index < sizes.len() - 1 {
                current_size_index += 1;
            }

            if start_time.elapsed() >= Duration::from_secs(10) {
                break;
            }
        }

        cancel_token.cancel();
    }

    async fn consumer(
        http_client: &Client,
        mut rx_size: Receiver<usize>,
        tx_bytes: Sender<u64>,
        cancel_token: CancellationToken,
        is_download: bool,
    ) -> Result<()> {
        while let Some(size) = rx_size.recv().await {
            if cancel_token.is_cancelled() {
                break;
            }

            if is_download {
                let url = format!("https://speed.cloudflare.com/__down?bytes={}", size);
                BandwidthTestClient::measure(
                    http_client,
                    &url,
                    tx_bytes.clone(),
                    cancel_token.clone(),
                )
                .await?;
            } else {
                let url = "https://speed.cloudflare.com/__up";
                let stream = DummyVecStream::new(size, 8192, tx_bytes.clone());

                http_client
                    .post(url)
                    .body(reqwest::Body::wrap_stream(stream))
                    .send()
                    .await?;
                let bytes = size as u64;
                tx_bytes.send(bytes).await.unwrap();
            }
        }

        Ok(())
    }

    async fn reporter(
        mut rx_bytes: Receiver<u64>,
        cancel_token: CancellationToken,
        label: &str,
    ) -> Result<()> {
        let bar = ProgressBar::new_spinner();
        bar.set_style(ProgressStyle::with_template("{msg} {spinner:.green}").unwrap());
        bar.set_message(format!("{:>20}: {} Mbps", label.bold(), "----.--".dimmed()));

        let mut total_bytes: u64 = 0;
        let start_time = Instant::now();
        let mut tick_counter = 0;

        while let Some(bytes) = rx_bytes.recv().await {
            total_bytes += bytes;

            let elapsed_time = start_time.elapsed().as_millis() as i32;
            let speed = measure_speed(total_bytes, elapsed_time);
            bar.set_message(format!(
                "{:>20}: {} Mbps",
                label.bold(),
                format!("{:.2}", speed).purple(),
            ));

            tick_counter += 1;
            if tick_counter % 10 == 0 {
                bar.tick();
            }

            if cancel_token.is_cancelled() {
                break;
            }
        }

        bar.finish_and_clear();
        println!(
            "{:>20}: {:.2} Mbps (Used: {} MiB)",
            label.bold(),
            measure_speed(total_bytes, start_time.elapsed().as_millis() as i32),
            total_bytes / 1024 / 1024
        );

        Ok(())
    }

    async fn measure(
        http_client: &Client,
        url: &str,
        tx_bytes: Sender<u64>,
        cancel_token: CancellationToken,
    ) -> Result<()> {
        let mut stream = http_client.get(url).send().await?.bytes_stream();
        while let Some(item) = stream.next().await {
            let bytes = item?.len() as u64;
            tx_bytes.send(bytes).await?;

            if cancel_token.is_cancelled() {
                break;
            }
        }

        Ok(())
    }

    pub async fn measure_download(&self) -> Result<Vec<f64>> {
        self.measure_bandwidth(true).await
    }

    pub async fn measure_upload(&self) -> Result<Vec<f64>> {
        self.measure_bandwidth(false).await
    }

    async fn measure_bandwidth(&self, is_download: bool) -> Result<Vec<f64>> {
        let sizes: &[usize] = if is_download {
            &[1001000, 10001000, 25001000, 100001000] // 1MB, 10MB, 25MB, 100MB
        } else {
            &[1000000, 10000000, 25000000] // 1MB, 10MB, 25MB
        };
        let label = if is_download {
            "Download Speed"
        } else {
            "Upload Speed"
        };

        let measurements = Vec::new();
        let (tx_size, rx_size): (Sender<usize>, Receiver<usize>) = tokio::sync::mpsc::channel(100);
        let (tx_bytes, rx_bytes): (Sender<u64>, Receiver<u64>) = tokio::sync::mpsc::channel(100);
        let cancel_token = CancellationToken::new();
        // not sure this is right
        let cancel_token_clone1 = cancel_token.clone();
        let cancel_token_clone2 = cancel_token.clone();
        let cancel_token_clone3 = cancel_token.clone();

        tokio::spawn(Self::producer(sizes, tx_size, cancel_token_clone1));

        let http_client = self.http_client.clone();
        tokio::spawn(async move {
            BandwidthTestClient::consumer(
                &http_client,
                rx_size,
                tx_bytes.clone(),
                cancel_token_clone2,
                is_download,
            )
            .await
            .unwrap_or_else(|err| {
                eprintln!("Error in consumer: {:?}", err);
            });
        });
        tokio::spawn(Self::reporter(rx_bytes, cancel_token_clone3, label));

        tokio::time::sleep(Duration::from_secs(20)).await;
        cancel_token.cancel();

        Ok(measurements)
    }
}

pub fn measure_speed(bytes: u64, ms: i32) -> f64 {
    if ms == 0 {
        return 0.0;
    }
    let bits = bytes as f64 * 8.0;
    let seconds = ms as f64 / 1000.0;
    bits / seconds / 1_000_000.0
}

struct DummyVecStream {
    max_size: usize,
    chunk_size: usize,
    cursor: usize,
    tx_bytes: Sender<u64>,
}

impl DummyVecStream {
    fn new(max_size: usize, chunk_size: usize, tx_bytes: Sender<u64>) -> Self {
        Self {
            max_size,
            chunk_size,
            cursor: 0,
            tx_bytes,
        }
    }
}

impl Stream for DummyVecStream {
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.cursor >= self.max_size {
            Poll::Ready(None)
        } else {
            let end = (self.cursor + self.chunk_size).min(self.max_size);
            let chunk = Bytes::copy_from_slice(&vec![0; end - self.cursor]);

            self.cursor = end;
            let _ = self.tx_bytes.try_send(chunk.len() as u64);

            Poll::Ready(Some(Ok(chunk)))
        }
    }
}
