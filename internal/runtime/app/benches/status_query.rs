#![allow(clippy::expect_used, clippy::unwrap_used)]

#[path = "../tests/common/mod.rs"]
mod common;

use std::time::{Duration, Instant};

use lilo_rm_core::{RuntimeResponse, StatusFilter};
use uuid::Uuid;

const DEFAULT_SAMPLES: usize = 100;
const P50_LIMIT: Duration = Duration::from_millis(5);

fn main() {
    let samples = sample_count();
    let harness = common::RtmHarness::start();
    let session_id = Uuid::now_v7().to_string();
    common::spawn_ok(&harness, &session_id, "claude");
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    let mut latencies = Vec::with_capacity(samples);

    for _ in 0..samples {
        let started = Instant::now();
        let response = runtime
            .block_on(lilo_runtime_app::shared::status_filtered(
                harness.socket_path(),
                StatusFilter::empty(),
            ))
            .expect("status rpc");
        assert!(matches!(response, RuntimeResponse::Status(_)));
        latencies.push(started.elapsed());
    }

    latencies.sort();
    let p50 = latencies[latencies.len() / 2];
    println!("status_query_p50_ms={:.3}", p50.as_secs_f64() * 1_000.0);
    assert!(p50 < P50_LIMIT, "status p50 {p50:?} exceeded {P50_LIMIT:?}");
}

fn sample_count() -> usize {
    std::env::var("LILO_BENCH_SAMPLES")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_SAMPLES)
}
