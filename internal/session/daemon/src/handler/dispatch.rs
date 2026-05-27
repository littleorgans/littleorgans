use anyhow::Result;
use lilo_im_core::{Action, ResourceSpec};
use lilo_session_core::{McpBridgeResponse, RpcRequest, RpcResponse, ShutdownResponse};
use uuid::Uuid;

use crate::identity_client::RequestContext;

use super::{DaemonState, HandlerResult};

impl DaemonState {
    pub async fn handle(&self, context: RequestContext, request: RpcRequest) -> HandlerResult {
        match request {
            RpcRequest::McpBridge { request } => {
                let context = match request.caller_session_id.as_deref() {
                    Some(raw) => match Uuid::parse_str(raw) {
                        Ok(id) => context.with_mcp_caller_session_id(id),
                        Err(error) => {
                            return HandlerResult {
                                response: RpcResponse::Error {
                                    message: format!("invalid MCP caller session id: {error}"),
                                },
                                shutdown: false,
                            };
                        }
                    },
                    None => context,
                };
                HandlerResult {
                    response: RpcResponse::McpBridge {
                        response: McpBridgeResponse {
                            line: crate::mcp_bridge::handle_line(self, &context, &request.line)
                                .await,
                        },
                    },
                    shutdown: false,
                }
            }
            request => self.handle_direct(context, request).await,
        }
    }

    pub(crate) async fn handle_direct(
        &self,
        context: RequestContext,
        request: RpcRequest,
    ) -> HandlerResult {
        match request {
            RpcRequest::Spawn { request } => response(self.spawn(&context, *request).await, false),
            RpcRequest::List { request } => response(self.list(request).await, false),
            RpcRequest::NamespaceCreate { request } => {
                response(self.create_namespace(request).await, false)
            }
            RpcRequest::NamespaceGet { request } => {
                response(self.get_namespace(request).await, false)
            }
            RpcRequest::NamespaceList { request } => {
                response(self.list_namespaces(request).await, false)
            }
            RpcRequest::NamespaceDelete { request } => {
                response(self.delete_namespace(context, request).await, false)
            }
            RpcRequest::Delete { request } => response(self.delete(&context, request).await, false),
            RpcRequest::MailSend { request } => {
                response(self.mail_send(&context, request).await, false)
            }
            RpcRequest::MailRead { request } => {
                response(self.mail_read(&context, request).await, false)
            }
            RpcRequest::MailCheck { request } => response(self.mail_check(&request).await, false),
            RpcRequest::MailStopCheck { request } => {
                response(self.mail_stop_check(&request).await, false)
            }
            RpcRequest::Nudge { request } => response(self.nudge(&context, request).await, false),
            RpcRequest::Label { request } => response(self.label(&context, request).await, false),
            RpcRequest::Logs { request } => response(self.logs(&context, request).await, false),
            RpcRequest::Capture { request } => {
                response(self.capture(&context, request).await, false)
            }
            RpcRequest::Doctor { request } => response(self.doctor(&context, request).await, false),
            RpcRequest::Wait { request } => response(self.wait(request).await, false),
            RpcRequest::McpBridge { .. } => response(
                Err(anyhow::anyhow!(
                    "nested MCP bridge requests are not supported"
                )),
                false,
            ),
            RpcRequest::Shutdown => response(self.shutdown(&context).await, true),
        }
    }

    async fn shutdown(&self, context: &RequestContext) -> Result<RpcResponse> {
        self.identity
            .authorize(&context.principal, Action::Daemon, &ResourceSpec::default())
            .await?;
        Ok(RpcResponse::Shutdown {
            response: ShutdownResponse {
                message: "stopping".to_string(),
            },
        })
    }
}

fn response(result: Result<RpcResponse>, shutdown_on_success: bool) -> HandlerResult {
    match result {
        Ok(response) => HandlerResult {
            response,
            shutdown: shutdown_on_success,
        },
        Err(error) => HandlerResult {
            response: RpcResponse::Error {
                message: format!("{error:#}"),
            },
            shutdown: false,
        },
    }
}
