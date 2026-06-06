#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn docker_unavailable_fails_before_lifecycle_insert() {
    let (state, testdb) = test_state().await;
    let session_id = SessionId::from_uuid(uuid::Uuid::now_v7());
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
    testdb.cleanup().await.expect("test db cleans up");
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn docker_tmux_pattern_a_passes_preflight() {
    let (state, testdb) = test_state().await;
    let mut request = tmux_request(SessionId::from_uuid(uuid::Uuid::now_v7()), false);
    request.isolation = docker_profile(None);

    let response = check_with_docker_inspector(
        &state,
        &mut request,
        &FakeDockerInspector::available_non_root(),
    )
    .await
    .expect("docker tmux target should pass preflight");

    assert!(response.is_none(), "tmux Docker attach returned conflict");
    testdb.cleanup().await.expect("test db cleans up");
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn docker_pattern_e_profile_fails_before_lifecycle_insert() {
    assert_docker_profile_rejected(
        "pattern-e",
        "isolation policy docker profile that requests a multiplexer inside the container is not supported",
    )
    .await;
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn docker_privileged_profile_fails_before_lifecycle_insert() {
    assert_docker_profile_rejected(
        "privileged",
        "isolation policy docker:privileged (requests privileged execution) is not supported",
    )
    .await;
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn unsupported_docker_profile_fails_before_lifecycle_insert() {
    assert_docker_profile_rejected(
        "locked",
        "isolation policy docker:locked (is not an accepted Docker profile) is not supported",
    )
    .await;
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn accepted_docker_profiles_probe_daemon_availability() {
    for profile in [None, Some("default"), Some("own-init")] {
        let (state, testdb) = test_state().await;
        let mut request = headless_request(SessionId::from_uuid(uuid::Uuid::now_v7()), false);
        request.isolation = docker_profile(profile);

        let response = check_with_docker_inspector(
            &state,
            &mut request,
            &FakeDockerInspector::available_non_root(),
        )
        .await
        .expect("preflight");

        assert!(response.is_none(), "accepted profile returned conflict");
        testdb.cleanup().await.expect("test db cleans up");
    }
}
