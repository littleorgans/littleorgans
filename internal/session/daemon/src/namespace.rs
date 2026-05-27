use anyhow::{Context, Result, bail};
use chrono::Utc;
use lilo_im_core::Action;
use lilo_im_core::ResourceSpec;
use lilo_session_core::{
    DeleteRequest, Namespace, NamespaceCreateRequest, NamespaceCreateResponse,
    NamespaceDeleteRequest, NamespaceDeleteResponse, NamespaceGetRequest, NamespaceGetResponse,
    NamespaceListRequest, NamespaceListResponse, RpcResponse, Selector,
};

use crate::handler::DaemonState;
use crate::identity_client::RequestContext;

impl DaemonState {
    pub(crate) fn create_namespace(&self, request: NamespaceCreateRequest) -> Result<RpcResponse> {
        let namespace = Namespace::for_create(request.slug)?;
        let record = {
            let store = self.store()?;
            let created = if store
                .namespace_exists(&namespace)
                .context("failed to check namespace")?
            {
                false
            } else {
                store
                    .create_namespace(&namespace, Utc::now())
                    .context("failed to create namespace")?;
                true
            };
            let record = store
                .list_namespaces()
                .context("failed to list namespaces")?
                .into_iter()
                .find(|record| record.namespace == namespace)
                .with_context(|| {
                    format!("created namespace is missing from catalog: {namespace}")
                })?;
            (record, created)
        };

        Ok(RpcResponse::NamespaceCreated {
            response: NamespaceCreateResponse {
                namespace: record.0,
                created: record.1,
            },
        })
    }

    pub(crate) fn list_namespaces(&self, _request: NamespaceListRequest) -> Result<RpcResponse> {
        let namespaces = self
            .store()?
            .list_namespaces()
            .context("failed to list namespaces")?;

        Ok(RpcResponse::NamespacesListed {
            response: NamespaceListResponse { namespaces },
        })
    }

    pub(crate) fn get_namespace(&self, request: NamespaceGetRequest) -> Result<RpcResponse> {
        let namespace = Namespace::new(request.slug)?;
        let namespace = self
            .store()?
            .list_namespaces()
            .context("failed to list namespaces")?
            .into_iter()
            .find(|record| record.namespace == namespace);

        Ok(RpcResponse::NamespaceGot {
            response: NamespaceGetResponse { namespace },
        })
    }

    pub(crate) async fn delete_namespace(
        &self,
        context: RequestContext,
        request: NamespaceDeleteRequest,
    ) -> Result<RpcResponse> {
        let namespace = request.namespace;
        namespace.ensure_not_default()?;
        self.identity
            .authorize(&context.principal, Action::Kill, &ResourceSpec::default())
            .await?;
        if !self.namespace_exists(&namespace)? {
            bail!("unknown namespace: {namespace}");
        }

        let sessions = self
            .cascade_terminate_namespace(&context, &namespace)
            .await?;
        self.remove_namespace_catalog(&namespace)?;

        Ok(RpcResponse::NamespaceDeleted {
            response: NamespaceDeleteResponse {
                namespace,
                sessions,
            },
        })
    }

    async fn cascade_terminate_namespace(
        &self,
        context: &RequestContext,
        namespace: &Namespace,
    ) -> Result<Vec<lilo_session_core::Session>> {
        let targets = self
            .store()?
            .list_sessions_by_selector(&Selector::Namespace {
                namespace: namespace.clone(),
            })
            .context("failed to list namespace sessions for cascade terminate")?;
        let request = DeleteRequest {
            selector: Selector::Namespace {
                namespace: namespace.clone(),
            },
            signal: "SIGTERM".to_string(),
            grace_secs: 5,
        };
        let mut sessions = Vec::new();
        let mut errors = Vec::new();
        for target in targets {
            match self.delete_one(context, &request, target.id).await {
                Ok(session) => sessions.push(session),
                Err(error) => errors.push(format!("{}: {error}", target.id)),
            }
        }
        if !errors.is_empty() {
            bail!(
                "failed to cascade terminate namespace {namespace}: {}",
                errors.join("; ")
            );
        }
        Ok(sessions)
    }

    fn remove_namespace_catalog(&self, namespace: &Namespace) -> Result<()> {
        let store = self.store()?;
        let active = store
            .active_session_count_in_namespace(namespace)
            .context("failed to verify namespace sessions before catalog removal")?;
        if active > 0 {
            bail!("namespace delete raced with session spawn: {namespace}");
        }
        store
            .delete_sessions_by_namespace(namespace)
            .context("failed to remove namespace sessions from catalog")?;
        let removed = store
            .delete_namespace(namespace)
            .context("failed to remove namespace catalog entry")?;
        if !removed {
            bail!("unknown namespace: {namespace}");
        }
        Ok(())
    }

    fn namespace_exists(&self, namespace: &Namespace) -> Result<bool> {
        self.store()?
            .namespace_exists(namespace)
            .context("failed to check namespace")
    }
}
