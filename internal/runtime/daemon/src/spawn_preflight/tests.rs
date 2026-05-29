use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::Utc;
use lilo_rm_core::{
    HeadlessSpawnTarget, IsolationPolicy, IsolationProfile, LaunchEnv, LaunchSpec, Lifecycle,
    MountSpec, RuntimeKind, ShimReady, SpawnConflictPayload, SpawnRequest, SpawnTarget,
    TmuxSpawnTarget, WatcherCounts,
};
use lilo_runtime_store::{LifecycleStore, StoreConfig};
use uuid::Uuid;

use super::*;
use crate::docker_preflight::DockerPreflightConfig;
use crate::reconcile::ReconcileConfig;
use crate::server::{DaemonConfig, ServerState};

include!("tests/helpers.rs");
include!("tests/conflicts.rs");
include!("tests/docker_profiles.rs");
include!("tests/mounts.rs");
include!("tests/image_user.rs");
include!("tests/image_availability.rs");
