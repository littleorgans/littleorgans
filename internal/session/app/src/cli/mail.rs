use anyhow::{Result, bail};
use lilo_paths::env::LILO_AGENT_SESSION_ID;
use std::{
    future::Future,
    str::FromStr,
    time::{Duration, Instant},
};

use lilo_session_core::{
    CallerContextRequest, MailIntent, MailLogCursor, MailLogFilter, MailNotifyMode,
    MailPeekRequest, MailReadRequest, MailSendRequest, MailStopCheckRequest, MailTailRequest,
    MailTailResponse, RpcResponse, SessionRpc,
};

use crate::cli::cli_def::{
    MailAction, MailArgs, MailObservationArgs, MailReadArgs, MailSendArgs, MailStopCheckArgs,
    MailTailArgs,
};
use crate::cli::output::{
    print_conversation_overview, print_mail_send_summary, print_messages, print_messages_short_ids,
};
use crate::cli::selector_scope::{required_scoped_selector, scoped_selector};

pub async fn run(args: MailArgs, json_output: bool) -> Result<()> {
    match args.action {
        MailAction::Send(args) => send(args, json_output).await,
        MailAction::Read(args) => read(args, json_output).await,
        MailAction::Peek(args) => peek(args, json_output).await,
        MailAction::StopCheck(args) => stop_check(args, json_output).await,
        MailAction::Tail(args) => tail(args, json_output).await,
    }
}

async fn send(args: MailSendArgs, json_output: bool) -> Result<()> {
    let response = send_daemon_request(SessionRpc::MailSend {
        request: MailSendRequest {
            to: required_scoped_selector(&args.to, &args.scope)?,
            content: args.content,
            notify: args
                .notify
                .as_deref()
                .map(MailNotifyMode::from_str)
                .transpose()?,
            context_id: args.context_id,
            intent: MailIntent::from_client_send_str(&args.intent)?,
            idempotency_key: args.idempotency_key,
        },
    })
    .await?;

    match response {
        RpcResponse::MailSent { response } => {
            if json_output {
                print_json(&RpcResponse::MailSent {
                    response: response.clone(),
                })?;
            } else {
                print_mail_send_summary(&response.results);
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

async fn read(_args: MailReadArgs, json_output: bool) -> Result<()> {
    let response = send_daemon_request(SessionRpc::MailRead {
        request: MailReadRequest {},
    })
    .await?;

    match response {
        RpcResponse::MailRead { response } => {
            if json_output {
                print_json(&RpcResponse::MailRead { response })?;
            } else {
                print_messages(&response.messages);
                print_errors(&response.errors);
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

async fn peek(args: MailObservationArgs, json_output: bool) -> Result<()> {
    let response = send_daemon_request(SessionRpc::MailPeek {
        request: MailPeekRequest {
            filter: observation_filter(&args)?,
        },
    })
    .await?;

    match response {
        RpcResponse::MailPeek { response } => {
            if json_output {
                print_json(&RpcResponse::MailPeek { response })?;
            } else if should_print_conversation_overview(&args) {
                print_conversation_overview(&response.messages);
            } else {
                let short_ids = crate::cli::short_ids::load().await?;
                print_messages_short_ids(&response.messages, &short_ids);
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

async fn stop_check(args: MailStopCheckArgs, json_output: bool) -> Result<()> {
    let response = send_daemon_request(SessionRpc::MailStopCheck {
        request: MailStopCheckRequest {
            selector: required_scoped_selector(&args.selector, &args.scope)?,
        },
    })
    .await?;
    let unread = match response {
        RpcResponse::MailStopChecked { response } => {
            let unread = response.unread;
            if json_output {
                print_json(&RpcResponse::MailStopChecked { response })?;
            }
            unread
        }
        RpcResponse::Error { message } => bail!(message),
        other => bail!(
            "unexpected daemon response: {} (please report)",
            other.kind()
        ),
    };
    if unread == 0 {
        return Ok(());
    }

    if json_output {
        std::process::exit(2);
    }

    println!(
        "{}",
        serde_json::json!({
            "decision": "block",
            "reason": format!("{unread} unread message(s). Run `sm mail read` to drain mail."),
        })
    );
    std::process::exit(2);
}

async fn tail(args: MailTailArgs, json_output: bool) -> Result<()> {
    let filter = observation_filter(&args.observation)?;
    let mut after = None;
    let mode = tail_mode(args.timeout, json_output, Instant::now());
    loop {
        let Some(response) =
            tail_once_until(filter.clone(), after.clone(), mode.follow, mode.deadline).await?
        else {
            if json_output {
                print_json(&RpcResponse::MailTail {
                    response: MailTailResponse {
                        messages: Vec::new(),
                        cursor: after,
                    },
                })?;
            }
            return Ok(());
        };
        after = response.cursor.clone().or(after);
        if json_output {
            print_json(&RpcResponse::MailTail { response })?;
        } else {
            print_messages(&response.messages);
        }
        if mode.single_shot {
            return Ok(());
        }
    }
}

#[derive(Debug)]
struct TailMode {
    follow: bool,
    single_shot: bool,
    deadline: Option<Instant>,
}

fn tail_mode(timeout: Option<u64>, json_output: bool, now: Instant) -> TailMode {
    TailMode {
        follow: timeout != Some(0),
        single_shot: json_output || timeout == Some(0),
        deadline: timeout
            .and_then(|seconds| (seconds > 0).then(|| now + Duration::from_secs(seconds))),
    }
}

async fn tail_once_until(
    filter: MailLogFilter,
    after: Option<MailLogCursor>,
    follow: bool,
    deadline: Option<Instant>,
) -> Result<Option<MailTailResponse>> {
    wait_until_deadline(deadline, tail_once(filter, after, follow)).await
}

async fn wait_until_deadline<T>(
    deadline: Option<Instant>,
    future: impl Future<Output = Result<T>>,
) -> Result<Option<T>> {
    let Some(deadline) = deadline else {
        return future.await.map(Some);
    };
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Ok(None);
    }
    match tokio::time::timeout(remaining, future).await {
        Ok(response) => response.map(Some),
        Err(_) => Ok(None),
    }
}

async fn tail_once(
    filter: MailLogFilter,
    after: Option<MailLogCursor>,
    follow: bool,
) -> Result<MailTailResponse> {
    let response = send_daemon_request(SessionRpc::MailTail {
        request: MailTailRequest {
            filter,
            after,
            follow,
            wait_ms: None,
        },
    })
    .await?;
    match response {
        RpcResponse::MailTail { response } => Ok(response),
        RpcResponse::Error { message } => bail!(message),
        other => bail!(
            "unexpected daemon response: {} (please report)",
            other.kind()
        ),
    }
}

fn observation_filter(args: &MailObservationArgs) -> Result<MailLogFilter> {
    let recipient = if args.recipient.is_some() {
        scoped_selector(args.recipient.as_deref(), &args.scope)?
    } else {
        None
    };
    Ok(MailLogFilter {
        context_id: args.context_id.clone(),
        selector: scoped_selector(args.selector.as_deref(), &args.scope)?,
        recipient,
        include_system: args.include_system,
    })
}

fn should_print_conversation_overview(args: &MailObservationArgs) -> bool {
    args.context_id.is_none()
        && args.selector.is_none()
        && args.recipient.is_none()
        && !args.include_system
}

fn print_json(response: &RpcResponse) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(response)?);
    Ok(())
}

fn print_errors(errors: &[lilo_session_core::TargetError]) {
    for error in errors {
        eprintln!("{} {}", error.target, error.message);
    }
}

async fn send_daemon_request(request: SessionRpc) -> Result<RpcResponse> {
    let request = request_with_caller_session(request)?;
    crate::cli::client::send_request(&request).await
}

fn request_with_caller_session(request: SessionRpc) -> Result<SessionRpc> {
    let Some(raw) = std::env::var_os(LILO_AGENT_SESSION_ID) else {
        return Ok(request);
    };
    let Ok(caller_session_id) = raw.into_string() else {
        bail!("{LILO_AGENT_SESSION_ID} is not valid UTF-8");
    };
    Ok(SessionRpc::CallerContext {
        request: CallerContextRequest {
            caller_session_id,
            request: Box::new(request),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_mode_maps_timeout_contract() {
        let now = Instant::now();

        let follow = tail_mode(None, false, now);
        assert!(follow.follow);
        assert!(!follow.single_shot);
        assert!(follow.deadline.is_none());

        let single_fetch = tail_mode(Some(0), false, now);
        assert!(!single_fetch.follow);
        assert!(single_fetch.single_shot);
        assert!(single_fetch.deadline.is_none());

        let bounded = tail_mode(Some(2), false, now);
        assert!(bounded.follow);
        assert!(!bounded.single_shot);
        assert!(bounded.deadline.is_some());

        let json = tail_mode(Some(2), true, now);
        assert!(json.follow);
        assert!(json.single_shot);
        assert!(json.deadline.is_some());
    }

    #[tokio::test]
    async fn deadline_bounds_pending_tail_await() {
        let deadline = Instant::now() + Duration::from_millis(1);
        let result = wait_until_deadline(Some(deadline), std::future::pending::<Result<()>>())
            .await
            .expect("deadline wait should not fail");

        assert!(result.is_none());
    }
}
