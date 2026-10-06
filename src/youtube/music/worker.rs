//! One bounded JSON line per request. Dropping an in-flight exchange kills its helper.
use super::*;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout},
    task::JoinHandle,
};

pub(super) struct Worker {
    pub(super) last_used: Instant,
    _child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    diagnostics: JoinHandle<Result<Vec<u8>>>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.diagnostics.abort();
    }
}

impl Worker {
    pub(super) fn spawn(python: &std::path::Path, script: &str) -> Result<Self> {
        let mut child = hidden_command(python)
            .args(["-I", "-u", "-c", script, "--worker"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Could not start YouTube Music; run 'tuitify youtube music-setup'")?;
        let input = child
            .stdin
            .take()
            .context("YouTube Music input unavailable")?;
        let output = BufReader::new(
            child
                .stdout
                .take()
                .context("YouTube Music output unavailable")?,
        );
        let stderr = child
            .stderr
            .take()
            .context("YouTube Music diagnostics unavailable")?;
        Ok(Self {
            last_used: Instant::now(),
            _child: child,
            input,
            output,
            diagnostics: tokio::spawn(bounded_read(stderr, 64 * 1024)),
        })
    }

    pub(super) async fn exchange(&mut self, bytes: &[u8]) -> Result<Value> {
        self.last_used = Instant::now();
        self.input.write_all(bytes).await?;
        self.input.write_all(b"\n").await?;
        self.input.flush().await?;
        let read = async {
            let mut bytes = Vec::new();
            loop {
                let buffer = self.output.fill_buf().await?;
                ensure!(
                    !buffer.is_empty(),
                    "YouTube Music helper disconnected; retry the request"
                );
                let newline = buffer.iter().position(|byte| *byte == b'\n');
                let count = newline.map_or(buffer.len(), |index| index + 1);
                ensure!(
                    bytes.len() + count <= MAX_OUTPUT as usize,
                    "YouTube Music response exceeded its size limit"
                );
                bytes.extend_from_slice(&buffer[..count]);
                self.output.consume(count);
                if newline.is_some() {
                    break;
                }
            }
            serde_json::from_slice(&bytes).map_err(|_| failure(FailureKind::InvalidResponse).into())
        };
        let result = tokio::select! {
            biased;
            result = read => result,
            _ = &mut self.diagnostics => anyhow::bail!("YouTube Music helper failed; retry the request or run 'tuitify youtube music-setup'. Diagnostic details were omitted."),
        };
        self.last_used = Instant::now();
        result
    }
}
