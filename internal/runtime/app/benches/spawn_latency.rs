#![allow(clippy::expect_used, clippy::unwrap_used)]

#[path = "../tests/common/mod.rs"]
mod common;

use std::time::{Duration, Instant};

use lilo_rm_core::{RuntimeResponse, RuntimeRpc};
use uuid::Uuid;

const DEFAULT_SAMPLES: usize = 10;
const P50_LIMIT: Duration = Duration::from_millis(200);

fn main() {
    let samples = common::bench_sample_count(DEFAULT_SAMPLES);
    let harness = common::RtmHarness::start();
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    let mut latencies = Vec::with_capacity(samples);

    for _ in 0..samples {
        let session_id = Uuid::now_v7();
        let started = Instant::now();
        let response = runtime
            .block_on(lilo_runtime_app::shared::request(
                harness.socket_path(),
                RuntimeRpc::Spawn {
                    request: common::headless_spawn_request(session_id, harness.rtm_home()),
                },
            ))
            .expect("spawn rpc");
        assert!(matches!(response, RuntimeResponse::Spawned(_)));
        latencies.push(started.elapsed());
    }

    latencies.sort();
    let p50 = latencies[latencies.len() / 2];
    println!("spawn_latency_p50_ms={:.3}", p50.as_secs_f64() * 1_000.0);
    assert!(p50 < P50_LIMIT, "spawn p50 {p50:?} exceeded {P50_LIMIT:?}");
}
