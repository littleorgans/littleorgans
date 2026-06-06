#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn root_image_user_fails_before_lifecycle_insert() {
    assert_docker_image_user_rejected(Some("root")).await;
    assert_docker_image_user_rejected(Some("0:0")).await;
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn missing_image_user_metadata_is_treated_as_root() {
    assert_docker_image_user_rejected(None).await;
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn root_image_user_is_allowed_by_profile_escape_hatch() {
    let (state, testdb) = test_state().await;
    let mut request = headless_request(SessionId::from_uuid(uuid::Uuid::now_v7()), false);
    request.isolation = docker_profile(Some("allow-root"));

    let response = check_with_docker_inspector(
        &state,
        &mut request,
        &FakeDockerInspector {
            availability: Ok(()),
            user: Ok(Some("root")),
            arm64_manifest: Ok(true),
            image_architecture: Ok("arm64"),
        },
    )
    .await
    .expect("preflight");

    assert!(response.is_none(), "allow-root profile returned conflict");
    testdb.cleanup().await.expect("test db cleans up");
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn root_image_user_is_allowed_by_config_escape_hatch() {
    let (state, testdb) =
        test_state_with_docker_config(DockerPreflightConfig::new("test-agent:latest", true, false))
            .await;
    let mut request = headless_request(SessionId::from_uuid(uuid::Uuid::now_v7()), false);
    request.isolation = docker_profile(None);

    let response = check_with_docker_inspector(
        &state,
        &mut request,
        &FakeDockerInspector {
            availability: Ok(()),
            user: Ok(Some("0")),
            arm64_manifest: Ok(true),
            image_architecture: Ok("arm64"),
        },
    )
    .await
    .expect("preflight");

    assert!(response.is_none(), "config root escape returned conflict");
    testdb.cleanup().await.expect("test db cleans up");
}
