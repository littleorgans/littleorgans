use anyhow::{Result, bail};

use lilo_session_core::{ListRequest, RpcResponse, Selector, SessionRpc};

use crate::cli::cli_def::{GetArgs, GetResource, SessionGetArgs, SessionListArgs};
use crate::cli::output::{print_session_line, print_session_table};
use crate::cli::selector_scope::scoped_selector;

pub async fn run(args: GetArgs, json_output: bool) -> Result<()> {
    match args.resource {
        GetResource::Session(args) if args.id.is_some() => get_session(args, json_output).await,
        GetResource::Session(args) => list_sessions(args.into(), json_output).await,
        GetResource::Namespace(args) => crate::cli::namespace::get(args.slug, json_output).await,
    }
}

async fn get_session(args: SessionGetArgs, json_output: bool) -> Result<()> {
    let id = args
        .id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("sm get session requires a session id"))?;
    let response = send_list(scoped_selector(Some(id), &args.read.scope)?).await?;

    match response {
        RpcResponse::Listed { response } if json_output => {
            let session = response
                .sessions
                .into_iter()
                .next()
                .ok_or_else(|| anyhow::anyhow!("unknown session: {id}"))?;
            println!("{}", serde_json::to_string_pretty(&session)?);
            Ok(())
        }
        RpcResponse::Listed { response } => {
            let session = response
                .sessions
                .first()
                .ok_or_else(|| anyhow::anyhow!("unknown session: {id}"))?;
            print_session_line(session, args.read.show_labels);
            Ok(())
        }
        RpcResponse::Error { message } => bail!(message),
        other => bail!(
            "unexpected daemon response: {} (please report)",
            other.kind()
        ),
    }
}

async fn list_sessions(args: SessionListArgs, json_output: bool) -> Result<()> {
    let response = send_list(scoped_selector(
        args.read.selector.as_deref(),
        &args.read.scope,
    )?)
    .await?;

    match response {
        RpcResponse::Listed { response } if json_output => {
            println!("{}", serde_json::to_string_pretty(&response.sessions)?);
            Ok(())
        }
        RpcResponse::Listed { response } => {
            print_session_table(&response.sessions, args.read.show_labels);
            Ok(())
        }
        RpcResponse::Error { message } => bail!(message),
        other => bail!(
            "unexpected daemon response: {} (please report)",
            other.kind()
        ),
    }
}

impl From<SessionGetArgs> for SessionListArgs {
    fn from(args: SessionGetArgs) -> Self {
        Self { read: args.read }
    }
}

async fn send_list(selector: Option<Selector>) -> Result<RpcResponse> {
    crate::cli::client::send_request(&SessionRpc::List {
        request: ListRequest { selector },
    })
    .await
}
