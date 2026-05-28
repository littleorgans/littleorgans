use std::str::FromStr;

use anyhow::{Context, Result, anyhow};
use lilo_session_core::{Label, MountSpec, Namespace, NamespaceScope, RpcResponse, Selector};
use serde_json::Value;

use crate::handler::DaemonState;
use crate::identity_client::RequestContext;

pub(super) async fn scoped_optional_selector(
    state: &DaemonState,
    context: &RequestContext,
    arguments: &Value,
    selector: Option<Selector>,
) -> Result<Option<Selector>> {
    Ok(
        match read_namespace_scope(state, context, arguments).await? {
            Some((namespace, scope)) => {
                Some(Selector::scoped_to_namespace(selector, namespace, scope)?)
            }
            None => selector,
        },
    )
}

pub(super) async fn scoped_required_selector(
    state: &DaemonState,
    context: &RequestContext,
    arguments: &Value,
    selector: Selector,
) -> Result<Selector> {
    scoped_optional_selector(state, context, arguments, Some(selector))
        .await?
        .ok_or_else(|| anyhow!("required selector was removed by namespace scoping"))
}

async fn read_namespace_scope(
    state: &DaemonState,
    context: &RequestContext,
    arguments: &Value,
) -> Result<Option<(Namespace, NamespaceScope)>> {
    if optional_bool(arguments, "all_namespaces").unwrap_or(false) {
        return Ok(None);
    }
    if let Some(raw) = optional_string(arguments, "namespace") {
        return Ok(Some((Namespace::from_str(raw)?, NamespaceScope::Explicit)));
    }
    if let Some(id) = context.mcp_caller_session_id {
        let session = state
            .store()
            .get_session(&id)
            .await
            .context("failed to load MCP caller session")?;
        if let Some(session) = session {
            return Ok(Some((session.namespace, NamespaceScope::Default)));
        }
    }
    Ok(Some((Namespace::default(), NamespaceScope::Default)))
}

pub(super) fn required_string<'a>(arguments: &'a Value, field: &str) -> Result<&'a str> {
    optional_string(arguments, field).ok_or_else(|| anyhow!("missing required argument `{field}`"))
}

pub(super) fn optional_string<'a>(arguments: &'a Value, field: &str) -> Option<&'a str> {
    arguments.get(field).and_then(Value::as_str)
}

pub(super) fn optional_u64(arguments: &Value, field: &str) -> Option<u64> {
    arguments.get(field).and_then(Value::as_u64)
}

pub(super) fn optional_bool(arguments: &Value, field: &str) -> Option<bool> {
    arguments.get(field).and_then(Value::as_bool)
}

pub(super) fn optional_mounts(arguments: &Value) -> Result<Vec<MountSpec>> {
    optional_string_array(arguments, "mounts", "HOST:CONTAINER[:ro|:rw]", |mount| {
        MountSpec::from_str(mount).map_err(Into::into)
    })
}

pub(super) fn required_selector(arguments: &Value, field: &str) -> Result<Selector> {
    Selector::from_str(required_string(arguments, field)?).map_err(Into::into)
}

pub(super) fn optional_selector(arguments: &Value, field: &str) -> Result<Option<Selector>> {
    optional_string(arguments, field)
        .map(Selector::from_str)
        .transpose()
        .map_err(Into::into)
}

pub(super) fn selector_from_id(id: &str) -> Result<Selector> {
    Ok(Selector::Id {
        id: uuid::Uuid::parse_str(id)?,
    })
}

pub(super) fn optional_labels(arguments: &Value) -> Result<Vec<Label>> {
    optional_string_array(arguments, "labels", "key=value", |label| {
        Label::from_str(label).map_err(Into::into)
    })
}

fn optional_string_array<T>(
    arguments: &Value,
    field: &str,
    entry_description: &str,
    parse: impl Fn(&str) -> Result<T>,
) -> Result<Vec<T>> {
    let Some(value) = arguments.get(field) else {
        return Ok(Vec::new());
    };
    value
        .as_array()
        .ok_or_else(|| anyhow!("`{field}` must be an array of {entry_description} strings"))?
        .iter()
        .map(|value| {
            let item = value
                .as_str()
                .ok_or_else(|| anyhow!("`{field}` entries must be strings"))?;
            parse(item)
        })
        .collect()
}

pub(super) fn unexpected_response(response: &RpcResponse) -> anyhow::Error {
    anyhow!(
        "unexpected daemon response: {} (please report)",
        response.kind()
    )
}
