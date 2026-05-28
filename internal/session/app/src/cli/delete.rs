use std::fs;

use anyhow::{Context, Result, bail};
use lilo_session_core::{
    DeleteRequest, Namespace, NamespaceDeleteRequest, RpcResponse, SessionRpc,
};

use crate::cli::cli_def::{DeleteArgs, DeleteNamespaceArgs, DeleteResource, DeleteSessionArgs};
use crate::cli::output::print_session_line;
use crate::cli::selector_scope::required_scoped_selector;

pub async fn run(args: DeleteArgs) -> Result<()> {
    match args.resource {
        DeleteResource::Session(args) => delete_session(args).await,
        DeleteResource::Namespace(args) => delete_namespace(args).await,
    }
}

async fn delete_session(args: DeleteSessionArgs) -> Result<()> {
    let response = crate::cli::client::send_request(&SessionRpc::Delete {
        request: DeleteRequest {
            selector: required_scoped_selector(&args.selector, &args.scope)?,
            signal: args.signal,
            grace_secs: args.grace,
        },
    })
    .await?;

    match response {
        RpcResponse::Deleted { response } => {
            for session in response.sessions {
                print_session_line(&session, false);
            }
            for error in response.errors {
                eprintln!("{} {}", error.target, error.message);
            }
            Ok(())
        }
        RpcResponse::Error { message } => bail!(message),
        other => bail!(
            "unexpected daemon response: {} (please report)",
            other.kind()
        ),
    }
}

async fn delete_namespace(args: DeleteNamespaceArgs) -> Result<()> {
    let namespace = args.namespace;
    namespace.ensure_not_default()?;
    let response = crate::cli::client::send_request(&SessionRpc::NamespaceDelete {
        request: NamespaceDeleteRequest {
            namespace: namespace.clone(),
        },
    })
    .await;

    match response {
        Ok(RpcResponse::NamespaceDeleted { response }) => {
            clear_binding_if_matches(&namespace).with_context(|| {
                format!(
                    "failed to clear namespace binding after deleting catalog entry: {namespace}. \
                     Remove the binding file manually or retry `sm delete namespace {namespace}`."
                )
            })?;
            for session in response.sessions {
                print_session_line(&session, false);
            }
            println!("deleted namespace: {}", response.namespace);
            Ok(())
        }
        Ok(RpcResponse::Error { message }) if message.contains("unknown namespace:") => {
            if clear_binding_if_matches(&namespace)? {
                println!("catalog entry already absent; stale binding cleared: {namespace}");
                Ok(())
            } else {
                bail!(message)
            }
        }
        Ok(RpcResponse::Error { message }) => bail!(message),
        Ok(other) => bail!(
            "unexpected daemon response: {} (please report)",
            other.kind()
        ),
        Err(error) => Err(error),
    }
}

fn clear_binding_if_matches(namespace: &Namespace) -> Result<bool> {
    let paths = crate::cli::client::paths_from_env()?;
    let binding = paths.namespace_binding();
    let contents = match fs::read_to_string(&binding) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", binding.display()));
        }
    };
    if contents.trim() != namespace.as_str() {
        return Ok(false);
    }
    fail_binding_clear_for_tests()?;
    fs::remove_file(&binding).with_context(|| format!("failed to remove {}", binding.display()))?;
    Ok(true)
}

fn fail_binding_clear_for_tests() -> Result<()> {
    #[cfg(debug_assertions)]
    if std::env::var_os("LILO_FAULT_NAMESPACE_BINDING_CLEAR").is_some() {
        bail!("fault injected while clearing namespace binding");
    }
    Ok(())
}
