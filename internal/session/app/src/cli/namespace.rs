use anyhow::{Result, bail};
use lilo_session_core::{
    NamespaceCreateRequest, NamespaceCreateResponse, NamespaceGetRequest, NamespaceListRequest,
    NamespaceRecord, RpcRequest, RpcResponse, SmEndpoint,
};

use crate::cli::cli_def::{CreateArgs, CreateResource, NamespaceCreateArgs};

pub async fn create(args: CreateArgs) -> Result<()> {
    match args.resource {
        CreateResource::Namespace(args) => create_namespace(args).await,
        CreateResource::Session(args) => crate::cli::run::create_session(args).await,
    }
}

pub async fn get(slug: Option<String>, json: bool) -> Result<()> {
    match slug {
        Some(slug) => get_one(slug, json).await,
        None => list(json).await,
    }
}

async fn list(json: bool) -> Result<()> {
    let response = send(&RpcRequest::NamespaceList {
        request: NamespaceListRequest::default(),
    })
    .await?;

    match response {
        RpcResponse::NamespacesListed { response } if json => {
            println!("{}", serde_json::to_string_pretty(&response.namespaces)?);
            Ok(())
        }
        RpcResponse::NamespacesListed { response } => {
            print_namespaces(&response.namespaces);
            Ok(())
        }
        RpcResponse::Error { message } => bail!(message),
        other => bail!(
            "unexpected daemon response: {} (please report)",
            other.kind()
        ),
    }
}

async fn get_one(slug: String, json: bool) -> Result<()> {
    let response = get_namespace_response(slug.clone()).await?;

    match response {
        RpcResponse::NamespaceGot { response } if json => {
            let namespace = response
                .namespace
                .ok_or_else(|| anyhow::anyhow!("unknown namespace: {slug}"))?;
            println!("{}", serde_json::to_string_pretty(&namespace)?);
            Ok(())
        }
        RpcResponse::NamespaceGot { response } => {
            let namespace = response
                .namespace
                .ok_or_else(|| anyhow::anyhow!("unknown namespace: {slug}"))?;
            print_namespaces(&[namespace]);
            Ok(())
        }
        RpcResponse::Error { message } => bail!(message),
        other => bail!(
            "unexpected daemon response: {} (please report)",
            other.kind()
        ),
    }
}

pub(crate) async fn get_namespace_record(slug: String) -> Result<Option<NamespaceRecord>> {
    match get_namespace_response(slug).await? {
        RpcResponse::NamespaceGot { response } => Ok(response.namespace),
        RpcResponse::Error { message } => bail!(message),
        other => bail!(
            "unexpected daemon response: {} (please report)",
            other.kind()
        ),
    }
}

async fn get_namespace_response(slug: String) -> Result<RpcResponse> {
    send(&RpcRequest::NamespaceGet {
        request: NamespaceGetRequest { slug },
    })
    .await
}

async fn create_namespace(args: NamespaceCreateArgs) -> Result<()> {
    let response = request_create(args.slug).await?;
    print_create_response(&response);
    Ok(())
}

async fn request_create(slug: String) -> Result<NamespaceCreateResponse> {
    let response = send(&RpcRequest::NamespaceCreate {
        request: NamespaceCreateRequest { slug },
    })
    .await?;

    match response {
        RpcResponse::NamespaceCreated { response } => Ok(response),
        RpcResponse::Error { message } => bail!(message),
        other => bail!(
            "unexpected daemon response: {} (please report)",
            other.kind()
        ),
    }
}

async fn send(request: &RpcRequest) -> Result<RpcResponse> {
    let endpoint = SmEndpoint::from_env()?;
    lilo_session_daemon::send_request(&endpoint, request).await
}

fn print_create_response(response: &NamespaceCreateResponse) {
    if response.created {
        println!("created namespace: {}", response.namespace.namespace);
    } else {
        println!("namespace already exists: {}", response.namespace.namespace);
    }
}

fn print_namespaces(namespaces: &[NamespaceRecord]) {
    println!("NAMESPACE CREATED_AT");
    for record in namespaces {
        println!("{} {}", record.namespace, record.created_at.to_rfc3339());
    }
}
