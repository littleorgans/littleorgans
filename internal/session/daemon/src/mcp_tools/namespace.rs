use anyhow::{Result, anyhow};
use lilo_session_core::{
    NamespaceGetRequest, NamespaceListRequest, RpcRequest, RpcResponse, tool_success,
};
use serde_json::{Value, json};

use crate::handler::DaemonState;
use crate::identity_client::RequestContext;

use super::args::{optional_string, required_string, unexpected_response};

pub(crate) async fn namespace_list(
    state: &DaemonState,
    context: &RequestContext,
    arguments: &Value,
) -> Result<Value> {
    if let Some(slug) = optional_string(arguments, "slug") {
        let namespace = namespace_record_by_slug(state, context, slug).await?;
        return Ok(tool_success(
            "1 namespace(s)".to_string(),
            &json!({ "namespaces": [namespace] }),
        ));
    }
    let response = state
        .handle_direct(
            context.clone(),
            RpcRequest::NamespaceList {
                request: NamespaceListRequest::default(),
            },
        )
        .await;
    match response.response {
        RpcResponse::NamespacesListed { response } => {
            let count = response.namespaces.len();
            Ok(tool_success(
                format!("{count} namespace(s)"),
                &json!({ "namespaces": response.namespaces }),
            ))
        }
        RpcResponse::Error { message } => Err(anyhow!(message)),
        other => Err(unexpected_response(&other)),
    }
}

pub(crate) async fn namespace_get(
    state: &DaemonState,
    context: &RequestContext,
    arguments: &Value,
) -> Result<Value> {
    let slug = required_string(arguments, "slug")?;
    let namespace = namespace_record_by_slug(state, context, slug).await?;
    Ok(tool_success(
        format!("found {}", namespace.namespace),
        &json!({ "namespace": namespace }),
    ))
}

async fn namespace_record_by_slug(
    state: &DaemonState,
    context: &RequestContext,
    slug: &str,
) -> Result<lilo_session_core::NamespaceRecord> {
    let response = state
        .handle_direct(
            context.clone(),
            RpcRequest::NamespaceGet {
                request: NamespaceGetRequest {
                    slug: slug.to_string(),
                },
            },
        )
        .await;
    match response.response {
        RpcResponse::NamespaceGot { response } => {
            let namespace = response
                .namespace
                .ok_or_else(|| anyhow!("unknown namespace: {slug}"))?;
            Ok(namespace)
        }
        RpcResponse::Error { message } => Err(anyhow!(message)),
        other => Err(unexpected_response(&other)),
    }
}
