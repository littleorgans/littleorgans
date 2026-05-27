struct FakeDockerInspector {
    availability: Result<(), &'static str>,
    user: Result<Option<&'static str>, &'static str>,
    arm64_manifest: Result<bool, &'static str>,
    image_architecture: Result<&'static str, FakeDockerImageError>,
}

impl FakeDockerInspector {
    fn available_non_root() -> Self {
        Self {
            availability: Ok(()),
            user: Ok(Some("1000")),
            arm64_manifest: Ok(true),
            image_architecture: Ok("arm64"),
        }
    }
}

#[derive(Clone, Copy)]
enum FakeDockerImageError {
    Unavailable(&'static str),
    MetadataUnavailable(&'static str),
}

impl DockerImageInspector for FakeDockerInspector {
    async fn ensure_available(&self) -> Result<()> {
        self.availability.map_err(RuntimeFailure::docker_unavailable)
    }

    async fn image_user(&self, _image: &str) -> Result<Option<String>> {
        self.user
            .map(|user| user.map(ToOwned::to_owned))
            .map_err(RuntimeFailure::docker_image_unavailable)
    }

    async fn arm64_manifest_available(&self, _image: &str) -> Result<bool> {
        self.arm64_manifest
            .map_err(RuntimeFailure::docker_image_metadata_unavailable)
    }

    async fn image_architecture(&self, _image: &str) -> Result<String> {
        self.image_architecture
            .map(ToOwned::to_owned)
            .map_err(|error| match error {
                FakeDockerImageError::Unavailable(message) => {
                    RuntimeFailure::docker_image_unavailable(message)
                }
                FakeDockerImageError::MetadataUnavailable(message) => {
                    RuntimeFailure::docker_image_metadata_unavailable(message)
                }
            })
    }
}

async fn test_state() -> Arc<ServerState> {
    test_state_with_docker_config(DockerPreflightConfig::new(
        "runtime-matters-agent:latest",
        false,
        false,
    ))
    .await
}

async fn test_state_with_docker_config(
    docker_preflight: DockerPreflightConfig,
) -> Arc<ServerState> {
    let temp = std::env::temp_dir().join(format!("rtm-preflight-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&temp).expect("tempdir");
    let db = lilo_db::LiloDb::open_path(temp.join("rtm.sqlite"))
        .await
        .expect("store db");
    let store = LifecycleStore::open(&db);
    Arc::new(
        ServerState::new(
            DaemonConfig {
                endpoint: lilo_paths::RuntimeEndpoint::unix_socket(temp.join("rtm.sock")),
                shim_path: temp.join("rtm"),
                log_root: temp.join("logs"),
                store: StoreConfig {
                    db_path: temp.join("rtm.sqlite"),
                },
                reconcile: ReconcileConfig::default(),
                docker_preflight,
            },
            store,
        )
        .expect("state"),
    )
}

async fn insert_running_tmux(state: &Arc<ServerState>, session_id: Uuid, runtime_pid: u32) {
    let mut lifecycle = Lifecycle::forking(session_id, RuntimeKind::Claude);
    state
        .store()
        .insert_forking(&lifecycle)
        .await
        .expect("insert");
    lifecycle.mark_running(ShimReady {
        session_id,
        shim_pid: runtime_pid + 1,
        runtime_pid,
        start_time: Utc::now(),
        tmux_pane: Some(tmux_address()),
    });
    state
        .store()
        .update_lifecycle(&lifecycle)
        .await
        .expect("running");
}

fn headless_request(session_id: Uuid, force: bool) -> SpawnRequest {
    SpawnRequest {
        session_id,
        runtime: RuntimeKind::Claude,
        isolation: IsolationPolicy::default(),
        image: None,
        env: Vec::new(),
        mounts: Vec::new(),
        cwd: "/tmp".into(),
        target: SpawnTarget::Headless(HeadlessSpawnTarget {}),
        force,
        shell_resume: None,
    }
}

fn tmux_request(session_id: Uuid, force: bool) -> SpawnRequest {
    SpawnRequest {
        target: SpawnTarget::Tmux(TmuxSpawnTarget {
            address: tmux_address(),
        }),
        ..headless_request(session_id, force)
    }
}

fn tmux_address() -> lilo_rm_core::TmuxAddress {
    "rtm-test:0.1".parse().expect("tmux address")
}

fn docker_profile(name: Option<&str>) -> IsolationPolicy {
    IsolationPolicy::Docker(IsolationProfile {
        name: name.map(ToOwned::to_owned),
    })
}

async fn assert_docker_profile_rejected(profile: &str, message: &str) {
    let state = test_state().await;
    let session_id = Uuid::now_v7();
    let mut request = headless_request(session_id, false);
    request.isolation = docker_profile(Some(profile));

    let error = check_with_docker_inspector(
        &state,
        &mut request,
        &FakeDockerInspector::available_non_root(),
    )
    .await
    .expect_err("docker profile should fail preflight");

    assert_eq!(error.to_string(), message);
    assert_no_lifecycle_or_waiters(&state, session_id).await;
}

async fn assert_docker_image_user_rejected(user: Option<&'static str>) {
    let state = test_state().await;
    let session_id = Uuid::now_v7();
    let mut request = headless_request(session_id, false);
    request.isolation = docker_profile(None);

    let error = check_with_docker_inspector(
        &state,
        &mut request,
        &FakeDockerInspector {
            availability: Ok(()),
            user: Ok(user),
            arm64_manifest: Ok(true),
            image_architecture: Ok("arm64"),
        },
    )
    .await
    .expect_err("root image user should fail preflight");

    assert_eq!(
        error.to_string(),
        "docker image metadata is unavailable: docker image runtime-matters-agent:latest runs as root"
    );
    assert_no_lifecycle_or_waiters(&state, session_id).await;
}

async fn assert_arm64_manifest_success(
    arm64_manifest: Result<bool, &'static str>,
    image_architecture: Result<&'static str, FakeDockerImageError>,
) {
    let state = test_state_with_docker_config(DockerPreflightConfig::new(
        "test-agent:latest",
        false,
        false,
    ))
    .await;
    let mut request = headless_request(Uuid::now_v7(), false);
    request.isolation = docker_profile(None);

    validate_docker_image_metadata_on_arch(
        &state,
        &request,
        docker_profile_ref(&request),
        &FakeDockerInspector {
            availability: Ok(()),
            user: Ok(Some("1000")),
            arm64_manifest,
            image_architecture,
        },
        "aarch64",
    )
    .await
    .expect("arm64 image should pass preflight");
}

async fn assert_arm64_manifest_failure_with_architecture(
    arm64_manifest: Result<bool, &'static str>,
    image_architecture: Result<&'static str, FakeDockerImageError>,
    expected: RuntimeFailure,
) {
    let state = test_state_with_docker_config(DockerPreflightConfig::new(
        "test-agent:latest",
        false,
        false,
    ))
    .await;
    let session_id = Uuid::now_v7();
    let mut request = headless_request(session_id, false);
    request.isolation = docker_profile(None);

    let error = validate_docker_image_metadata_on_arch(
        &state,
        &request,
        docker_profile_ref(&request),
        &FakeDockerInspector {
            availability: Ok(()),
            user: Ok(Some("1000")),
            arm64_manifest,
            image_architecture,
        },
        "aarch64",
    )
    .await
    .expect_err("arm64 image should fail preflight");

    assert_runtime_failure(&error, expected);
    assert_no_lifecycle_or_waiters(&state, session_id).await;
}

async fn assert_arm64_manifest_failure(arm64_manifest: Result<bool, &'static str>, expected: &str) {
    let state = test_state_with_docker_config(DockerPreflightConfig::new(
        "test-agent:latest",
        false,
        false,
    ))
    .await;
    let session_id = Uuid::now_v7();
    let mut request = headless_request(session_id, false);
    request.isolation = docker_profile(None);

    let error = validate_docker_image_metadata_on_arch(
        &state,
        &request,
        docker_profile_ref(&request),
        &FakeDockerInspector {
            availability: Ok(()),
            user: Ok(Some("1000")),
            arm64_manifest,
            image_architecture: Err(FakeDockerImageError::Unavailable(
                "No such image: test-agent:latest",
            )),
        },
        "aarch64",
    )
    .await
    .expect_err("arm64 manifest should fail preflight");

    assert_eq!(error.to_string(), expected);
    assert_no_lifecycle_or_waiters(&state, session_id).await;
}

fn assert_runtime_failure(error: &anyhow::Error, expected: RuntimeFailure) {
    let failure = error
        .downcast_ref::<RuntimeFailure>()
        .expect("runtime failure category");
    match (failure, expected) {
        (
            RuntimeFailure::DockerImageUnavailable { message },
            RuntimeFailure::DockerImageUnavailable { message: expected },
        )
        | (
            RuntimeFailure::DockerImageMetadataUnavailable { message },
            RuntimeFailure::DockerImageMetadataUnavailable { message: expected },
        ) => assert_eq!(message, &expected),
        (actual, expected) => {
            panic!("unexpected failure category: {actual:?}, expected {expected:?}")
        }
    }
}

fn docker_profile_ref(request: &SpawnRequest) -> &IsolationProfile {
    let IsolationPolicy::Docker(profile) = &request.isolation else {
        panic!("request is not docker isolated");
    };
    profile
}

async fn assert_no_lifecycle_or_waiters(state: &Arc<ServerState>, session_id: Uuid) {
    assert!(
        state
            .store()
            .get(session_id)
            .await
            .expect("store")
            .is_none(),
        "preflight inserted lifecycle row"
    );
    assert_eq!(
        state.watcher_counts().await,
        WatcherCounts {
            process_exit_watchers: 0,
            shim_sockets: 0,
            event_waiters: 0,
        }
    );
}

fn assert_conflict(response: RuntimeResponse, kind: SpawnConflictKind, session_id: Uuid) {
    let RuntimeResponse::SpawnConflict(payload) = response else {
        panic!("unexpected response: {response:?}");
    };
    assert_eq!(payload.kind, kind);
    assert_eq!(payload.lifecycle.session_id, session_id);
}

fn wait_for_child_exit(child: &mut std::process::Child) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        match child.try_wait().expect("poll child") {
            Some(_) => return,
            None => std::thread::sleep(Duration::from_millis(25)),
        }
    }
    let _ = child.kill();
    panic!("child was still alive after force preemption");
}
