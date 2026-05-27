use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::str::FromStr;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use lilo_session_core::{LogsRequest, RpcResponse, Selector, SessionRpc, SmEndpoint};

use crate::cli::cli_def::LogsArgs;

pub async fn run(args: LogsArgs) -> Result<()> {
    let endpoint = SmEndpoint::from_env()?;
    let response = lilo_session_daemon::send_request(
        &endpoint,
        &SessionRpc::Logs {
            request: LogsRequest {
                selector: Selector::from_str(&args.selector)?,
                max_bytes: args.max_bytes,
            },
        },
    )
    .await?;

    match response {
        RpcResponse::Logs { response } => {
            print!("{}", response.content);
            std::io::stdout().flush().ok();
            if args.follow {
                let offset = std::fs::metadata(&response.transcript_path)
                    .map_or(response.content.len() as u64, |metadata| metadata.len());
                follow_transcript(&response.transcript_path, offset)?;
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

fn follow_transcript(path: &std::path::Path, mut offset: u64) -> Result<()> {
    loop {
        let metadata = std::fs::metadata(path)
            .with_context(|| format!("failed to stat {}", path.display()))?;
        if metadata.len() < offset {
            offset = 0;
        }
        if metadata.len() > offset {
            let mut file =
                File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
            file.seek(SeekFrom::Start(offset))?;
            let mut chunk = String::new();
            file.read_to_string(&mut chunk)?;
            print!("{chunk}");
            std::io::stdout().flush().ok();
            offset = metadata.len();
        }
        thread::sleep(Duration::from_millis(250));
    }
}
