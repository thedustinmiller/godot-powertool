use anyhow::{Context, Result, bail};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

const MAX_MSG_SIZE: usize = 64 * 1024 * 1024; // 64 MiB

/// Read one LSP message from a Content-Length framed stream.
/// Returns `Ok(None)` on clean EOF (stream closed before any header bytes).
pub async fn read_message<R: AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
) -> Result<Option<serde_json::Value>> {
    let mut content_length: Option<usize> = None;
    let mut line = String::new();

    // Parse headers until blank line
    loop {
        line.clear();
        let n = reader
            .read_line(&mut line)
            .await
            .context("reading LSP header")?;
        if n == 0 {
            // EOF before any header — clean shutdown
            return Ok(None);
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            // Blank line ends headers
            break;
        }

        if let Some(val) = trimmed.strip_prefix("Content-Length:") {
            content_length = Some(
                val.trim()
                    .parse::<usize>()
                    .context("invalid Content-Length value")?,
            );
        }
        // Ignore other headers (e.g. Content-Type)
    }

    let length = content_length.context("missing Content-Length header")?;
    if length > MAX_MSG_SIZE {
        bail!("Content-Length {length} exceeds {MAX_MSG_SIZE} byte limit");
    }

    let mut body = vec![0u8; length];
    reader
        .read_exact(&mut body)
        .await
        .context("reading LSP message body")?;

    let msg = serde_json::from_slice(&body).context("parsing LSP JSON body")?;
    Ok(Some(msg))
}

/// Write one LSP message with Content-Length framing. Flushes after write.
pub async fn write_message<W: AsyncWrite + Unpin>(
    writer: &mut W,
    msg: &serde_json::Value,
) -> Result<()> {
    let body = serde_json::to_vec(msg).context("serializing LSP message")?;
    let header = format!("Content-Length: {}\r\n\r\n", body.len());

    writer
        .write_all(header.as_bytes())
        .await
        .context("writing LSP header")?;
    writer
        .write_all(&body)
        .await
        .context("writing LSP body")?;
    writer.flush().await.context("flushing LSP message")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn round_trip() {
        let msg = serde_json::json!({"jsonrpc": "2.0", "method": "initialized"});

        let mut buf = Vec::new();
        write_message(&mut buf, &msg).await.unwrap();

        let mut reader = BufReader::new(buf.as_slice());
        let result = read_message(&mut reader).await.unwrap().unwrap();
        assert_eq!(result, msg);
    }

    #[tokio::test]
    async fn eof_returns_none() {
        let mut reader = BufReader::new(&b""[..]);
        let result = read_message(&mut reader).await.unwrap();
        assert!(result.is_none());
    }
}
