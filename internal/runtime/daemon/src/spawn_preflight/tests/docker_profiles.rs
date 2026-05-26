#[tokio::test]
async fn docker_unavailable_fails_before_lifecycle_insert() {
    let state = test_state().await;
    let session_id = Uuid::now_v7();
    let mut request = headless_request(session_id, false);
    request.isolation = docker_profile(None);

    let error = check_with_docker_inspector(
        &state,
        &mut request,
        &FakeDockerInspector {
            availability: Err("daemon socket refused"),
            user: Ok(Some("1000")),
            arm64_manifest: Ok(true),
            image_architecture: Ok("arm64"),
        },
    )
    .await
    .expect_err("docker unavailable should fail preflight");

    assert_eq!(
        error.to_string(),
        "docker daemon is unavailable: daemon socket refused"
    );
    assert_no_lifecycle_or_waiters(&state, session_id).await;
}

#[tokio::test]
async fn docker_tmux_pattern_a_passes_preflight() {
    let state = test_state().await;
    let mut request = tmux_request(Uuid::now_v7(), false);
    request.isolation = docker_profile(None);

    let response = check_with_docker_inspector(
        &state,
        &mut request,
        &FakeDockerInspector::available_non_root(),
    )
    .await
    .expect("docker tmux target should pass preflight");

    assert!(response.is_none(), "tmux Docker attach returned conflict");
}

#[tokio::test]
async fn docker_pattern_e_profile_fails_before_lifecycle_insert() {
    assert_docker_profile_rejected(
        "pattern-e",
        "isolation policy docker profile that requests a multiplexer inside the container is not supported",
    )
    .await;
}

#[tokio::test]
async fn docker_privileged_profile_fails_before_lifecycle_insert() {
    assert_docker_profile_rejected(
        "privileged",
        "isolation policy docker:privileged (requests privileged execution) is not supported",
    )
    .await;
}

#[tokio::test]
async fn unsupported_docker_profile_fails_before_lifecycle_insert() {
    assert_docker_profile_rejected(
        "locked",
        "isolation policy docker:locked (is not an accepted Docker profile) is not supported",
    )
    .await;
}

#[tokio::test]
async fn accepted_docker_profiles_probe_daemon_availability() {
    for profile in [None, Some("default"), Some("own-init")] {
        let state = test_state().await;
        let mut request = headless_request(Uuid::now_v7(), false);
        request.isolation = docker_profile(profile);

        let response = check_with_docker_inspector(
            &state,
            &mut request,
            &FakeDockerInspector::available_non_root(),
        )
        .await
        .expect("preflight");

        assert!(response.is_none(), "accepted profile returned conflict");
    }
}
