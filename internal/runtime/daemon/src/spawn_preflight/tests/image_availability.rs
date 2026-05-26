#[tokio::test]
async fn image_unavailable_fails_before_lifecycle_insert() {
    let state = test_state().await;
    let session_id = Uuid::now_v7();
    let mut request = headless_request(session_id, false);
    request.isolation = docker_profile(None);

    let error = check_with_docker_inspector(
        &state,
        &mut request,
        &FakeDockerInspector {
            availability: Ok(()),
            user: Err("pull access denied"),
            arm64_manifest: Ok(true),
            image_architecture: Ok("arm64"),
        },
    )
    .await
    .expect_err("image unavailable should fail preflight");

    assert_eq!(
        error.to_string(),
        "docker image is unavailable: pull access denied"
    );
    assert_no_lifecycle_or_waiters(&state, session_id).await;
}

#[tokio::test]
async fn arm64_manifest_absence_fails_before_lifecycle_insert() {
    assert_arm64_manifest_failure(
        Ok(false),
        "docker image metadata is unavailable: docker image test-agent:latest does not publish an arm64 manifest",
    )
    .await;
}

#[tokio::test]
async fn local_metadata_failure_returns_local_error_without_manifest_fallback() {
    assert_arm64_manifest_failure_with_architecture(
        Ok(true),
        Err(FakeDockerImageError::MetadataUnavailable(
            "invalid architecture metadata",
        )),
        RuntimeFailure::DockerImageMetadataUnavailable {
            message: "invalid architecture metadata".to_owned(),
        },
    )
    .await;
}

#[tokio::test]
async fn local_only_arm64_image_passes_on_arm64_host() {
    assert_arm64_manifest_success(Ok(false), Ok("arm64")).await;
}

#[tokio::test]
async fn local_only_non_arm64_image_fails_on_arm64_host() {
    assert_arm64_manifest_failure_with_architecture(
        Ok(true),
        Ok("amd64"),
        RuntimeFailure::DockerImageMetadataUnavailable {
            message: "docker image test-agent:latest does not publish an arm64 manifest".to_owned(),
        },
    )
    .await;
}

#[tokio::test]
async fn nonexistent_local_image_reports_image_unavailable_after_registry_failure() {
    assert_arm64_manifest_failure_with_architecture(
        Err("registry authentication required"),
        Err(FakeDockerImageError::Unavailable(
            "No such image: test-agent:latest",
        )),
        RuntimeFailure::DockerImageUnavailable {
            message: "No such image: test-agent:latest".to_owned(),
        },
    )
    .await;
}

#[tokio::test]
async fn arm64_manifest_escape_hatch_skips_manifest_inspection() {
    let state = test_state().await;
    let mut request = headless_request(Uuid::now_v7(), false);
    request.isolation = docker_profile(Some("arm64-manifest-escape"));

    validate_docker_image_metadata_on_arch(
        &state,
        &request,
        docker_profile_ref(&request),
        &FakeDockerInspector {
            availability: Ok(()),
            user: Ok(Some("1000")),
            arm64_manifest: Err("registry authentication required"),
            image_architecture: Err(FakeDockerImageError::Unavailable(
                "No such image: test-agent:latest",
            )),
        },
        "aarch64",
    )
    .await
    .expect("preflight");
}
