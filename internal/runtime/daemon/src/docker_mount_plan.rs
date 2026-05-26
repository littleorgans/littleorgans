use std::path::{Component, Path, PathBuf};

use anyhow::Result;
use lilo_rm_core::MountSpec;

use crate::error::RuntimeFailure;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DockerMount {
    pub(crate) source: PathBuf,
    pub(crate) target: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CwdCover {
    pub(crate) source: PathBuf,
    pub(crate) target: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CwdMountPlan {
    pub(crate) auto_mount_cwd: bool,
    pub(crate) workdir: String,
}

impl DockerMount {
    pub(crate) fn new(source: PathBuf, target: &Path) -> Result<Self> {
        let target = normalize_container_path(&path_string(target))
            .ok_or_else(|| invalid_container_path("docker mount target", &path_string(target)))?;
        Ok(Self { source, target })
    }
}

pub(crate) fn plan_cwd_mount(cwd_source: &Path, mounts: &[MountSpec]) -> Result<CwdMountPlan> {
    let mounts = mounts
        .iter()
        .map(|mount| DockerMount::new(mount.source.clone(), &mount.target))
        .collect::<Result<Vec<_>>>()?;
    validate_cwd_mount_plan(cwd_source, &mounts)
}

pub(crate) fn validate_cwd_mount_plan(
    cwd_source: &Path,
    mounts: &[DockerMount],
) -> Result<CwdMountPlan> {
    reject_cwd_source_descendants(cwd_source, mounts)?;
    let cover = select_cwd_cover(cwd_source, mounts)?;
    let cwd_target = normalize_container_path(&path_string(cwd_source))
        .ok_or_else(|| invalid_container_path("spawn cwd", &path_string(cwd_source)))?;

    if let Some(cover) = cover {
        Ok(CwdMountPlan {
            auto_mount_cwd: false,
            workdir: remap_cwd_workdir(&cover, cwd_source)?,
        })
    } else {
        reject_cwd_target_overlaps(&cwd_target, mounts)?;
        Ok(CwdMountPlan {
            auto_mount_cwd: true,
            workdir: cwd_target,
        })
    }
}

pub(crate) fn host_path_covers(root: &Path, value: &Path) -> bool {
    root == value || value.starts_with(root)
}

pub(crate) fn select_cwd_cover(
    cwd_source: &Path,
    mounts: &[DockerMount],
) -> Result<Option<CwdCover>> {
    let mut selected: Option<&DockerMount> = None;
    let mut selected_len = 0;

    for mount in mounts {
        if !host_path_covers(&mount.source, cwd_source) {
            continue;
        }

        let length = mount.source.components().count();
        if length > selected_len {
            selected = Some(mount);
            selected_len = length;
            continue;
        }

        if length == selected_len {
            let Some(previous) = selected else {
                selected = Some(mount);
                selected_len = length;
                continue;
            };
            return Err(RuntimeFailure::protocol_mismatch(format!(
                "multiple docker mount sources cover spawn cwd {} with equal precedence: {} and {}",
                cwd_source.display(),
                previous.source.display(),
                mount.source.display()
            )));
        }
    }

    Ok(selected.map(|mount| CwdCover {
        source: mount.source.clone(),
        target: mount.target.clone(),
    }))
}

pub(crate) fn reject_cwd_source_descendants(
    cwd_source: &Path,
    mounts: &[DockerMount],
) -> Result<()> {
    for mount in mounts {
        if host_path_covers(cwd_source, &mount.source) && mount.source != cwd_source {
            return Err(RuntimeFailure::protocol_mismatch(format!(
                "docker mount source {} overlaps the cwd auto-mount source {}",
                mount.source.display(),
                cwd_source.display()
            )));
        }
    }
    Ok(())
}

pub(crate) fn container_path_covers(root: &str, value: &str) -> bool {
    root == "/"
        || value == root
        || value
            .strip_prefix(root)
            .is_some_and(|tail| tail.starts_with('/'))
}

pub(crate) fn normalize_container_path(value: &str) -> Option<String> {
    if !value.starts_with('/') {
        return None;
    }

    let mut parts = Vec::new();
    for part in value.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }

    Some(if parts.is_empty() {
        "/".to_owned()
    } else {
        format!("/{}", parts.join("/"))
    })
}

fn reject_cwd_target_overlaps(cwd_target: &str, mounts: &[DockerMount]) -> Result<()> {
    for mount in mounts {
        if container_paths_overlap(cwd_target, &mount.target) {
            return Err(RuntimeFailure::protocol_mismatch(format!(
                "docker mount target {} overlaps the cwd auto-mount target {cwd_target}",
                mount.target
            )));
        }
    }
    Ok(())
}

fn remap_cwd_workdir(cover: &CwdCover, cwd_source: &Path) -> Result<String> {
    let relative = cwd_source.strip_prefix(&cover.source).map_err(|_| {
        RuntimeFailure::protocol_mismatch(format!(
            "docker mount source {} does not cover spawn cwd {}",
            cover.source.display(),
            cwd_source.display()
        ))
    })?;
    let mut workdir = cover.target.clone();

    for component in relative.components() {
        let Component::Normal(name) = component else {
            continue;
        };
        push_container_component(&mut workdir, &name.to_string_lossy());
    }

    Ok(workdir)
}

fn push_container_component(path: &mut String, component: &str) {
    if path == "/" {
        path.push_str(component);
        return;
    }
    path.push('/');
    path.push_str(component);
}

fn container_paths_overlap(left: &str, right: &str) -> bool {
    container_path_covers(left, right) || container_path_covers(right, left)
}

fn invalid_container_path(label: &str, value: &str) -> anyhow::Error {
    RuntimeFailure::protocol_mismatch(format!(
        "{label} {value} must be an absolute container path"
    ))
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
