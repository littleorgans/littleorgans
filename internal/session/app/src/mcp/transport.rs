use tokio::io::{AsyncWrite, AsyncWriteExt};

pub async fn write_line<W>(stdout: &mut W, line: &str) -> std::io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    stdout.write_all(line.as_bytes()).await?;
    stdout.write_all(b"\n").await?;
    stdout.flush().await
}
