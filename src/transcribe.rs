use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

#[derive(Deserialize)]
struct TranscriptionResponse {
    text: String,
}

/// A client for a local `whisper-server`. The server is long-lived and shared
/// across invocations so the model is loaded once and kept resident, which
/// makes repeat transcriptions fast.
pub struct ServerClient {
    base_url: String,
}

impl ServerClient {
    pub fn new(port: u16) -> Self {
        ServerClient {
            base_url: format!("http://127.0.0.1:{port}"),
        }
    }

    pub fn healthy(&self) -> bool {
        match reqwest::blocking::Client::new()
            .get(format!("{}/health", self.base_url))
            .send()
        {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    /// Send an audio file to the server and return the transcript text.
    ///
    /// `language` may be a specific code ("en", "pl") or "auto" for
    /// auto-detection. whisper-server decodes WAV/MP3/FLAC/OGG natively and
    /// resamples to 16 kHz itself; for other formats (e.g. m4a) convert to WAV
    /// first.
    pub fn transcribe(&self, audio_path: &Path, language: &str) -> Result<String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()?;

        let form = reqwest::blocking::multipart::Form::new()
            .text("language", language.to_string())
            .text("response_format", "json".to_string())
            .file("file", audio_path)
            .context("attaching audio file to request")?;

        let resp = client
            .post(format!("{}/inference", self.base_url))
            .multipart(form)
            .send()
            .context("request to whisper-server failed")?;

        if !resp.status().is_success() {
            return Err(anyhow!("whisper-server returned {}", resp.status()));
        }

        let parsed: TranscriptionResponse = resp.json()?;
        let text = parsed.text.trim().to_string();
        Ok(crate::text::cleanup(&text))
    }
}

/// Reuse a running server, or start one detached (so it survives this process
/// exiting) and wait until it is healthy. Returns a client to talk to it.
pub fn ensure_server_running(binary: &Path, model_path: &Path, port: u16) -> Result<ServerClient> {
    let client = ServerClient::new(port);
    if client.healthy() {
        log::info!("reusing running whisper-server on port {port}");
        return Ok(client);
    }

    log::info!("starting whisper-server (loads the model once; stays resident)");
    let log_path = std::env::temp_dir().join("voice-paste-whisper-server.log");
    spawn_detached(binary, model_path, port, &log_path)?;
    log::info!("server logs: {}", log_path.display());

    wait_for_health(&client, &log_path)?;
    Ok(client)
}

/// Launch the server in its own process group with stdio redirected to a log
/// file, so it keeps running after this process exits.
fn spawn_detached(binary: &Path, model_path: &Path, port: u16, log_path: &Path) -> Result<()> {
    let stdout_log = std::fs::File::create(log_path)?;
    let stderr_log = stdout_log.try_clone()?;

    let mut cmd = Command::new(binary);
    cmd.arg("-m")
        .arg(model_path)
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(port.to_string())
        .arg("-l")
        .arg("auto")
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_log))
        .stderr(Stdio::from(stderr_log));

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    cmd.spawn()
        .with_context(|| format!("failed to launch {}", binary.display()))?;
    Ok(())
}

fn wait_for_health(client: &ServerClient, log_path: &Path) -> Result<()> {
    // Model load (plus a one-time Metal kernel compile on macOS) can take a bit.
    let deadline = std::time::Instant::now() + Duration::from_secs(300);
    loop {
        if client.healthy() {
            return Ok(());
        }
        if std::time::Instant::now() > deadline {
            return Err(anyhow!(
                "whisper-server did not become healthy in time — check {} for details",
                log_path.display()
            ));
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// Convenience: default port for the local server.
pub fn default_port() -> u16 {
    std::env::var("VOICE_PASTE_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080)
}
