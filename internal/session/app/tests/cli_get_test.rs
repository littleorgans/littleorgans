mod common;

#[path = "cli_get_test/agent_config.rs"]
mod agent_config;
#[path = "cli_get_test/create_session.rs"]
mod create_session;
#[path = "cli_get_test/help.rs"]
mod help;
#[path = "cli_get_test/helpers.rs"]
mod helpers;
#[path = "cli_get_test/run_resolution.rs"]
mod run_resolution;
#[path = "cli_get_test/session_read.rs"]
mod session_read;

#[allow(unused_imports)]
pub(crate) use agent_config::{
    run_agent_config_paths_are_canonicalized_from_caller_context,
    run_missing_named_agent_config_surfaces_resolved_path,
};
#[allow(unused_imports)]
pub(crate) use create_session::{
    create_session_and_run_persist_compatible_records_for_shared_inputs,
    create_session_persists_headless_record_without_foreground_attach,
};
#[allow(unused_imports)]
pub(crate) use help::{
    create_help_lists_namespace_and_session_resources,
    create_session_help_exposes_only_declarative_arguments,
    get_namespace_help_exposes_only_namespace_read_arguments,
    get_session_help_exposes_only_session_read_arguments,
    run_help_exposes_force_as_imperative_argument,
};
#[allow(unused_imports)]
pub(crate) use helpers::{
    assert_success, assert_table_contains, canonical_display, first_field, get_session_json,
    stderr, stdout,
};
#[allow(unused_imports)]
pub(crate) use run_resolution::{
    run_persists_canonical_dir_from_cli_resolution,
    unknown_namespace_error_is_surfaced_from_daemon, workspace_arg_is_rejected_by_clap,
};
#[allow(unused_imports)]
pub(crate) use session_read::{
    capture_takes_exact_session_id, removed_get_forms_are_rejected_by_clap,
    session_resources_list_and_get_by_id,
};
