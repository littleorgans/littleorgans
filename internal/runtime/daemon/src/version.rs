use lilo_rm_core::VersionInfo;

pub(crate) fn runtime_version_info() -> VersionInfo {
    VersionInfo::new(
        env!("CARGO_PKG_VERSION"),
        lilo_rm_core::version_info().git_sha,
    )
}
