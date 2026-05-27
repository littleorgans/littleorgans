#![allow(clippy::expect_used, clippy::unwrap_used)]

#[path = "../tests/common/mod.rs"]
mod common;

use std::time::{Duration, Instant};

use lilo_rm_core::{
    HeadlessSpawnTarget, IsolationPolicy, RuntimeKind, RuntimeResponse, RuntimeRpc, SpawnRequest,
    SpawnTarget,
};
use uuid::Uuid;

const DEFAULT_SAMPLES: usize = 10;
const P50_LIMIT: Duration = Duration::from_millis(200);

fn main() {
    let samples = sample_count();
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
                    request: SpawnRequest {
                        session_id,
                        runtime: RuntimeKind::Claude,
                        isolation: IsolationPolicy::default(),
                        image: None,
                        env: Vec::new(),
                        mounts: Vec::new(),
                        cwd: harness.rtm_home().to_path_buf(),
                        target: SpawnTarget::Headless(HeadlessSpawnTarget {}),
                        force: false,
                        shell_resume: None,
                    },
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

fn sample_count() -> usize {
    std::env::var("LILO_BENCH_SAMPLES")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_SAMPLES)
}
