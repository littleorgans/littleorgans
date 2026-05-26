use anyhow::Result;
use lilo_rm_core::SpawnRequest;
use serde::Deserialize;
use tokio::process::Command;

use crate::error::RuntimeFailure;

const RTM_DOCKER_IMAGE: &str = "RTM_DOCKER_IMAGE";
const RTM_DOCKER_ALLOW_ROOT_IMAGE_USER: &str = "RTM_DOCKER_ALLOW_ROOT_IMAGE_USER";
const RTM_DOCKER_ALLOW_ARM64_MANIFEST_ESCAPE: &str = "RTM_DOCKER_ALLOW_ARM64_MANIFEST_ESCAPE";

#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub struct DockerPreflightConfig {
    image: Option<String>,
    allow_root_image_user: bool,
    allow_arm64_manifest_escape: bool,
}

impl DockerPreflightConfig {
    pub fn from_env() -> Self {
        Self {
            image: std::env::var(RTM_DOCKER_IMAGE)
                .ok()
                .filter(|value| !value.trim().is_empty())
                .map(|value| trimmed_string(&value)),
            allow_root_image_user: env_flag(RTM_DOCKER_ALLOW_ROOT_IMAGE_USER),
            allow_arm64_manifest_escape: env_flag(RTM_DOCKER_ALLOW_ARM64_MANIFEST_ESCAPE),
        }
    }

    #[cfg(test)]
    pub(crate) fn new(
        image: impl Into<String>,
        allow_root_image_user: bool,
        allow_arm64_manifest_escape: bool,
    ) -> Self {
        Self {
            image: Some(image.into()),
            allow_root_image_user,
            allow_arm64_manifest_escape,
        }
    }

    pub(crate) fn image_for<'a>(&'a self, request: &'a SpawnRequest) -> Result<&'a str> {
        request
            .image
            .as_deref()
            .or(self.image.as_deref())
            .ok_or_else(RuntimeFailure::docker_image_not_configured)
    }

    pub(crate) fn allows_root_image_user(&self) -> bool {
        self.allow_root_image_user
    }

    pub(crate) fn allows_arm64_manifest_escape(&self) -> bool {
        self.allow_arm64_manifest_escape
    }
}

fn trimmed_string(value: &str) -> String {
    value.trim().to_owned()
}

pub(crate) trait DockerImageInspector {
    async fn ensure_available(&self) -> Result<()>;
    async fn image_user(&self, image: &str) -> Result<Option<String>>;
    async fn arm64_manifest_available(&self, image: &str) -> Result<bool>;
    async fn image_architecture(&self, image: &str) -> Result<String>;
}

pub(crate) struct DockerCliInspector;

impl DockerImageInspector for DockerCliInspector {
    async fn ensure_available(&self) -> Result<()> {
        let output = Command::new("docker")
            .arg("version")
            .arg("--format")
            .arg("{{.Server.Version}}")
            .output()
            .await
            .map_err(|error| RuntimeFailure::docker_unavailable(error.to_string()))?;

        if output.status.success() {
            return Ok(());
        }

        Err(RuntimeFailure::docker_unavailable(command_error_message(
            &output.stderr,
            "docker version failed without stderr",
        )))
    }

    async fn image_user(&self, image: &str) -> Result<Option<String>> {
        let output = Command::new("docker")
            .arg("image")
            .arg("inspect")
            .arg(image)
            .arg("--format")
            .arg("{{json .Config.User}}")
            .output()
            .await
            .map_err(|error| RuntimeFailure::docker_image_unavailable(error.to_string()))?;

        if !output.status.success() {
            return Err(RuntimeFailure::docker_image_unavailable(
                command_error_message(&output.stderr, "docker image inspect failed without stderr"),
            ));
        }

        let raw = String::from_utf8_lossy(&output.stdout);
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed == "null" {
            return Ok(None);
        }
        let user = serde_json::from_str::<String>(trimmed).map_err(|error| {
            RuntimeFailure::docker_image_metadata_unavailable(format!(
                "docker image inspect returned invalid user metadata: {error}"
            ))
        })?;
        Ok(non_empty(&user))
    }

    async fn arm64_manifest_available(&self, image: &str) -> Result<bool> {
        let output = Command::new("docker")
            .arg("manifest")
            .arg("inspect")
            .arg(image)
            .output()
            .await
            .map_err(|error| {
                RuntimeFailure::docker_image_metadata_unavailable(error.to_string())
            })?;

        if !output.status.success() {
            return Err(RuntimeFailure::docker_image_metadata_unavailable(
                command_error_message(
                    &output.stderr,
                    "docker manifest inspect failed without stderr",
                ),
            ));
        }

        manifest_has_arm64(&output.stdout)
    }

    async fn image_architecture(&self, image: &str) -> Result<String> {
        let output = Command::new("docker")
            .arg("image")
            .arg("inspect")
            .arg(image)
            .arg("--format")
            .arg("{{json .Architecture}}")
            .output()
            .await
            .map_err(|error| RuntimeFailure::docker_image_unavailable(error.to_string()))?;

        if !output.status.success() {
            return Err(RuntimeFailure::docker_image_unavailable(
                command_error_message(&output.stderr, "docker image inspect failed without stderr"),
            ));
        }

        let raw = String::from_utf8_lossy(&output.stdout);
        let architecture = serde_json::from_str::<String>(raw.trim()).map_err(|error| {
            RuntimeFailure::docker_image_metadata_unavailable(format!(
                "docker image inspect returned invalid architecture metadata: {error}"
            ))
        })?;
        non_empty(&architecture).ok_or_else(|| {
            RuntimeFailure::docker_image_metadata_unavailable(
                "docker image inspect returned empty architecture metadata",
            )
        })
    }
}

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn command_error_message(stderr: &[u8], fallback: &str) -> String {
    let message = String::from_utf8_lossy(stderr).trim().to_owned();
    if message.is_empty() {
        fallback.to_owned()
    } else {
        message
    }
}

fn manifest_has_arm64(stdout: &[u8]) -> Result<bool> {
    let manifest = serde_json::from_slice::<DockerManifest>(stdout).map_err(|error| {
        RuntimeFailure::docker_image_metadata_unavailable(format!(
            "docker manifest inspect returned invalid metadata: {error}"
        ))
    })?;
    Ok(match manifest.manifests {
        Some(manifests) => manifests
            .iter()
            .any(|manifest| manifest.platform.architecture == "arm64"),
        None => true,
    })
}

#[derive(Deserialize)]
struct DockerManifest {
    manifests: Option<Vec<DockerManifestDescriptor>>,
}

#[derive(Deserialize)]
struct DockerManifestDescriptor {
    platform: DockerPlatform,
}

#[derive(Deserialize)]
struct DockerPlatform {
    architecture: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use lilo_rm_core::{
        HeadlessSpawnTarget, IsolationPolicy, RuntimeKind, ShellResume, SpawnTarget,
    };
    use uuid::Uuid;

    #[test]
    fn request_image_overrides_env_default() {
        let config = DockerPreflightConfig::new("env-default:latest", false, false);
        let mut request = spawn_request();
        request.image = Some("request-image:dev".to_owned());

        assert_eq!(
            config.image_for(&request).expect("image"),
            "request-image:dev"
        );
    }

    #[test]
    fn env_default_is_used_when_request_image_is_absent() {
        let config = DockerPreflightConfig::new("env-default:latest", false, false);

        assert_eq!(
            config.image_for(&spawn_request()).expect("image"),
            "env-default:latest"
        );
    }

    #[test]
    fn missing_request_and_env_image_is_typed_error() {
        let error = DockerPreflightConfig::default()
            .image_for(&spawn_request())
            .expect_err("missing image should fail");

        assert!(matches!(
            error.downcast_ref::<RuntimeFailure>(),
            Some(RuntimeFailure::DockerImageNotConfigured)
        ));
    }

    #[test]
    fn request_image_accepts_oci_reference_shapes() {
        let config = DockerPreflightConfig::default();

        for image in [
            "runtime-matters-claude",
            "runtime-matters-claude:local",
            "ghcr.io/org/runtime-matters-claude:1.2.3",
            "runtime-matters-claude@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ] {
            let mut request = spawn_request();
            request.image = Some(image.to_owned());
            assert_eq!(config.image_for(&request).expect("image"), image);
        }
    }

    #[test]
    fn manifest_list_reports_arm64_availability() {
        let manifest = br#"{
            "manifests": [
                { "platform": { "architecture": "amd64" } },
                { "platform": { "architecture": "arm64" } }
            ]
        }"#;

        assert!(manifest_has_arm64(manifest).expect("manifest"));
    }

    #[test]
    fn manifest_list_reports_known_arm64_absence() {
        let manifest = br#"{
            "manifests": [
                { "platform": { "architecture": "amd64" } }
            ]
        }"#;

        assert!(!manifest_has_arm64(manifest).expect("manifest"));
    }

    #[test]
    fn single_platform_manifest_is_not_a_known_arm64_absence() {
        let manifest = br#"{ "schemaVersion": 2 }"#;

        assert!(manifest_has_arm64(manifest).expect("manifest"));
    }

    fn spawn_request() -> SpawnRequest {
        SpawnRequest {
            session_id: Uuid::nil(),
            runtime: RuntimeKind::Claude,
            isolation: IsolationPolicy::Docker(lilo_rm_core::IsolationProfile::default()),
            image: None,
            env: Vec::new(),
            mounts: Vec::new(),
            cwd: "/tmp".into(),
            target: SpawnTarget::Headless(HeadlessSpawnTarget {}),
            force: false,
            shell_resume: None::<ShellResume>,
        }
    }
}
