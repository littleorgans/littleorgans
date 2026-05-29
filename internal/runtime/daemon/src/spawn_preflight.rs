use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Result;
use lilo_rm_core::{
    IsolationPolicy, IsolationProfile, KillRequest, LaunchEnv, Lifecycle, LifecycleState,
    MountSpec, RuntimeSignal, SpawnConflictKind, SpawnConflictPayload, SpawnRequest,
    claude_path_shaped_env,
};

use crate::server::ServerState;
use crate::{
    docker_mount_plan::{self, DockerMount, container_path_covers, normalize_container_path},
    docker_preflight::{DockerCliInspector, DockerImageInspector},
    error::RuntimeFailure,
};

pub(crate) async fn check(
    state: &Arc<ServerState>,
    request: &mut SpawnRequest,
) -> Result<Option<SpawnConflictPayload>> {
    check_with_docker_inspector(state, request, &DockerCliInspector).await
}

async fn check_with_docker_inspector(
    state: &Arc<ServerState>,
    request: &mut SpawnRequest,
    docker: &impl DockerImageInspector,
) -> Result<Option<SpawnConflictPayload>> {
    check_isolation_policy(state, request, docker).await?;

    if let Some(lifecycle) = state.store().get(request.session_id).await?
        && !is_session_backed_forking(&lifecycle, request)
    {
        return Ok(Some(conflict(SpawnConflictKind::SessionId, lifecycle)));
    }

    let Some(address) = request.target.tmux_address() else {
        return Ok(None);
    };
    let Some(occupant) = state.store().running_tmux_occupant(address).await? else {
        return Ok(None);
    };
    if !request.force {
        return Ok(Some(conflict(
            SpawnConflictKind::TmuxPaneOccupancy,
            occupant,
        )));
    }

    state
        .kill_runtime(KillRequest {
            session_id: occupant.session_id,
            signal: RuntimeSignal::Term,
            grace_secs: 2,
        })
        .await?;
    Ok(None)
}

async fn check_isolation_policy(
    state: &Arc<ServerState>,
    request: &mut SpawnRequest,
    docker: &impl DockerImageInspector,
) -> Result<()> {
    match request.isolation.clone() {
        IsolationPolicy::Host => {
            warn_host_mounts(request);
            Ok(())
        }
        IsolationPolicy::Docker(profile) => {
            check_docker_profile(state, request, &profile, docker).await
        }
    }
}

async fn check_docker_profile(
    state: &Arc<ServerState>,
    request: &mut SpawnRequest,
    profile: &IsolationProfile,
    docker: &impl DockerImageInspector,
) -> Result<()> {
    match profile.name.as_deref() {
        None | Some("default" | "own-init" | "allow-root" | "arm64-manifest-escape") => {
            validate_docker_mounts(request)?;
            validate_docker_image_metadata_on_arch(
                state,
                request,
                profile,
                docker,
                std::env::consts::ARCH,
            )
            .await
        }
        Some("pattern-e" | "tmux-primary") => Err(unsupported_docker_behavior(
            "requests a multiplexer inside the container",
        )),
        Some("privileged") => Err(unsupported_docker_profile(
            profile,
            "requests privileged execution",
        )),
        Some(_) => Err(unsupported_docker_profile(
            profile,
            "is not an accepted Docker profile",
        )),
    }
}

fn warn_host_mounts(request: &SpawnRequest) {
    if !request.mounts.is_empty() {
        tracing::warn!(
            %request.session_id,
            mount_count = request.mounts.len(),
            "host isolation ignores declared spawn mounts"
        );
    }
}

fn validate_docker_mounts(request: &mut SpawnRequest) -> Result<()> {
    let cwd_source = canonicalize_cwd(&request.cwd)?;
    request.cwd.clone_from(&cwd_source);
    let mounts = canonicalize_request_mounts(&request.cwd, &mut request.mounts)?;

    reject_duplicate_mount_targets(&mounts)?;
    docker_mount_plan::validate_cwd_mount_plan(&cwd_source, &mounts)?;
    reject_uncovered_path_envs(&request.env, &mounts)
}

fn canonicalize_request_mounts(cwd: &Path, mounts: &mut [MountSpec]) -> Result<Vec<DockerMount>> {
    mounts
        .iter_mut()
        .map(|mount| {
            let source = canonicalize_mount_source(cwd, &mount.source)?;
            mount.source.clone_from(&source);
            DockerMount::new(source, &mount.target)
        })
        .collect()
}

fn reject_duplicate_mount_targets(mounts: &[DockerMount]) -> Result<()> {
    let mut counts = BTreeMap::<&str, usize>::new();

    for mount in mounts {
        *counts.entry(&mount.target).or_default() += 1;
    }

    for (target, count) in counts {
        if count > 1 {
            return Err(RuntimeFailure::protocol_mismatch(format!(
                "docker mount target {target} is declared more than once"
            )));
        }
    }
    Ok(())
}

fn reject_uncovered_path_envs(env: &[LaunchEnv], mounts: &[DockerMount]) -> Result<()> {
    for entry in env.iter().filter_map(path_shaped_env) {
        for path_value in entry.catalog.path_values(&entry.env.value) {
            let normalized = normalize_container_path(path_value).ok_or_else(|| {
                RuntimeFailure::protocol_mismatch(format!(
                    "path-shaped env {}={} must be an absolute container path",
                    entry.env.key, entry.env.value
                ))
            })?;
            if !mounts
                .iter()
                .any(|mount| container_path_covers(&mount.target, &normalized))
            {
                return Err(RuntimeFailure::protocol_mismatch(format!(
                    "path-shaped env {}={} is not covered by a declared Docker mount target; add --mount {}:{}:ro",
                    entry.env.key, entry.env.value, path_value, path_value
                )));
            }
        }
    }
    Ok(())
}

struct PathEnvEntry<'a> {
    env: &'a LaunchEnv,
    catalog: &'static lilo_rm_core::PathShapedEnv,
}

fn path_shaped_env(env: &LaunchEnv) -> Option<PathEnvEntry<'_>> {
    if !env.key.starts_with("CLAUDE_") {
        return None;
    }
    claude_path_shaped_env(&env.key).map(|catalog| PathEnvEntry { env, catalog })
}

fn canonicalize_cwd(cwd: &Path) -> Result<PathBuf> {
    cwd.canonicalize().map_err(|error| {
        RuntimeFailure::protocol_mismatch(format!(
            "spawn cwd {} could not be canonicalized for Docker mount preflight: {error}",
            cwd.display()
        ))
    })
}

fn canonicalize_mount_source(cwd: &Path, source: &Path) -> Result<PathBuf> {
    let absolute = if source.is_absolute() {
        source.to_path_buf()
    } else {
        cwd.join(source)
    };

    absolute.canonicalize().map_err(|error| {
        RuntimeFailure::protocol_mismatch(format!(
            "docker mount source {} could not be canonicalized relative to cwd {}: {error}",
            source.display(),
            cwd.display()
        ))
    })
}

async fn validate_docker_image_metadata_on_arch(
    state: &Arc<ServerState>,
    request: &SpawnRequest,
    profile: &IsolationProfile,
    docker: &impl DockerImageInspector,
    host_arch: &str,
) -> Result<()> {
    docker.ensure_available().await?;
    let image = state.config().docker_preflight.image_for(request)?;
    let user = docker.image_user(image).await?;
    if image_user_is_root(user.as_deref()) && !docker_root_allowed(state, profile) {
        return Err(RuntimeFailure::docker_image_metadata_unavailable(format!(
            "docker image {image} runs as root"
        )));
    }
    if host_arch == "aarch64" && !docker_manifest_escape_allowed(state, profile) {
        let arm64_available = docker_image_arm64_available(docker, image).await?;
        if !arm64_available {
            return Err(RuntimeFailure::docker_image_metadata_unavailable(format!(
                "docker image {image} does not publish an arm64 manifest"
            )));
        }
    }
    Ok(())
}

async fn docker_image_arm64_available(
    docker: &impl DockerImageInspector,
    image: &str,
) -> Result<bool> {
    match docker.image_architecture(image).await {
        Ok(arch) => Ok(arch == "arm64"),
        Err(local_error) if is_docker_image_unavailable(&local_error) => {
            match docker.arm64_manifest_available(image).await {
                Ok(available) => Ok(available),
                Err(_) => Err(local_error),
            }
        }
        Err(local_error) => Err(local_error),
    }
}

fn is_docker_image_unavailable(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<RuntimeFailure>()
        .is_some_and(|failure| matches!(failure, RuntimeFailure::DockerImageUnavailable { .. }))
}

fn docker_root_allowed(state: &Arc<ServerState>, profile: &IsolationProfile) -> bool {
    state.config().docker_preflight.allows_root_image_user()
        || profile.name.as_deref() == Some("allow-root")
}

fn docker_manifest_escape_allowed(state: &Arc<ServerState>, profile: &IsolationProfile) -> bool {
    state
        .config()
        .docker_preflight
        .allows_arm64_manifest_escape()
        || profile.name.as_deref() == Some("arm64-manifest-escape")
}

fn image_user_is_root(user: Option<&str>) -> bool {
    let Some(user) = user else {
        return true;
    };
    let primary = user.split(':').next().unwrap_or(user).trim();
    primary.is_empty() || primary == "0" || primary == "root"
}

fn unsupported_docker_profile(profile: &IsolationProfile, reason: &str) -> anyhow::Error {
    RuntimeFailure::unsupported_isolation_policy(format!(
        "{} ({reason})",
        IsolationPolicy::Docker(profile.clone())
    ))
}

fn unsupported_docker_behavior(reason: &str) -> anyhow::Error {
    RuntimeFailure::unsupported_isolation_policy(format!("docker profile that {reason}"))
}

fn conflict(kind: SpawnConflictKind, lifecycle: lilo_rm_core::Lifecycle) -> SpawnConflictPayload {
    SpawnConflictPayload { kind, lifecycle }
}

fn is_session_backed_forking(lifecycle: &Lifecycle, request: &SpawnRequest) -> bool {
    lifecycle.state == LifecycleState::Forking
        && lifecycle.runtime == request.runtime
        && lifecycle.isolation == request.isolation
}

#[cfg(test)]
mod tests;
