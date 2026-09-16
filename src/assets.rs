use anyhow::{anyhow, bail, Context, Result};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Whisper models are hosted (un-gated) on the ggerganov/whisper.cpp HF repo.
const HF_RESOLVE_BASE: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

/// Default model: multilingual Whisper Large v3 Turbo — best speed/accuracy for
/// on-the-fly dictation, and handles both English and Polish.
pub const DEFAULT_MODEL: &str = "large-v3-turbo";

/// Models we know exist as `ggml-<name>.bin` in the repo above. Used to turn a
/// friendly `--model` value into a filename without surprising the user.
const KNOWN_MODELS: &[&str] = &[
    "tiny", "tiny.en", "base", "base.en", "small", "small.en", "medium", "medium.en",
    "large-v1", "large-v2", "large-v3", "large-v3-turbo", "large-v3-turbo-q5_0",
    "large-v3-turbo-q8_0", "large-v3-q5_0", "medium-q5_0", "medium-q8_0",
];

pub struct Assets {
    pub server_bin: PathBuf,
    pub model_path: PathBuf,
}

/// Make sure the `whisper-server` binary exists and the requested GGML model is
/// downloaded, downloading the model automatically if missing.
pub fn ensure(model_dir: &Path, model: &str) -> Result<Assets> {
    fs::create_dir_all(model_dir).with_context(|| format!("create {}", model_dir.display()))?;

    let server_bin = find_whisper_server()?;
    let model_path = model_path(model_dir, model)?;
    ensure_model(&model_path, model)?;

    Ok(Assets {
        server_bin,
        model_path,
    })
}

/// Resolve asset locations, honoring the env overrides.
pub fn default_paths() -> (PathBuf, PathBuf) {
    let cache = cache_root();
    let bin_dir = std::env::var_os("VOICE_PASTE_BIN_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| cache.join("bin"));
    let model_dir = std::env::var_os("VOICE_PASTE_MODEL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| cache.join("models").join("whisper"));
    (bin_dir, model_dir)
}

fn cache_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("VOICE_PASTE_CACHE_DIR") {
        return PathBuf::from(dir);
    }
    let home = home_dir();
    #[cfg(target_os = "macos")]
    {
        home.join("Library").join("Caches").join("voice-paste")
    }
    #[cfg(not(target_os = "macos"))]
    {
        home.join(".cache").join("voice-paste")
    }
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn model_path(model_dir: &Path, model: &str) -> Result<PathBuf> {
    if !KNOWN_MODELS.contains(&model) {
        bail!(
            "unknown whisper model '{model}'. Known models: {}",
            KNOWN_MODELS.join(", ")
        );
    }
    Ok(model_dir.join(format!("ggml-{model}.bin")))
}

// ---------------------------------------------------------------------------
// whisper-server binary discovery
// ---------------------------------------------------------------------------

fn binary_name() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "whisper-server.exe"
    }
    #[cfg(not(target_os = "windows"))]
    {
        "whisper-server"
    }
}

/// Locate the `whisper-server` binary. Look in (in order):
/// 1. `VOICE_PASTE_BIN_DIR`
/// 2. `$PATH`
/// 3. common Homebrew locations (macOS Apple Silicon / Intel, Linux)
fn find_whisper_server() -> Result<PathBuf> {
    let name = binary_name();

    if let Some(dir) = std::env::var_os("VOICE_PASTE_BIN_DIR") {
        let p = PathBuf::from(dir).join(name);
        if p.is_file() {
            return Ok(p);
        }
    }

    if let Some(p) = find_on_path(name) {
        return Ok(p);
    }

    for dir in ["/opt/homebrew/bin", "/usr/local/bin", "/home/linuxbrew/.linuxbrew/bin"] {
        let p = Path::new(dir).join(name);
        if p.is_file() {
            return Ok(p);
        }
    }

    Err(anyhow!(
        "could not find `{name}`. Install whisper.cpp (which provides the \
         `whisper-server` binary) and try again, e.g.:\n\
         \n  brew install whisper.cpp   # macOS / Linux (Homebrew)\n\
         \n\
         or set VOICE_PASTE_BIN_DIR to a directory containing `{name}`."
    ))
}

/// Search `$PATH` for an executable file named `name`.
fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Model download
// ---------------------------------------------------------------------------

fn ensure_model(model_path: &Path, model: &str) -> Result<()> {
    // Cheap sanity check: a real model is far larger than an error page.
    if let Ok(md) = fs::metadata(model_path) {
        if md.len() > 100 * 1024 * 1024 {
            return Ok(());
        }
        let _ = fs::remove_file(model_path);
    }

    let url = format!("{HF_RESOLVE_BASE}/ggml-{model}.bin");
    log::info!(
        "downloading Whisper model '{}' (this is a one-time download)…",
        model
    );
    download(&url, model_path)?;
    log::info!("model saved to {}", model_path.display());
    Ok(())
}

fn download(url: &str, dest: &Path) -> Result<()> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60 * 60))
        .build()?;

    let mut resp = client
        .get(url)
        .send()
        .with_context(|| format!("GET {url}"))?;

    if !resp.status().is_success() {
        bail!("download failed ({}): {url}", resp.status());
    }

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut out = fs::File::create(dest)?;

    let total = resp.content_length();
    let mut copied: u64 = 0;
    let mut next_report: u64 = 50 * 1024 * 1024;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = resp.read(&mut buf)?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut out, &buf[..n])?;
        copied += n as u64;
        if copied >= next_report {
            match total {
                Some(t) if t > 0 => log::info!(
                    "  downloaded {}/{} MiB",
                    copied / (1024 * 1024),
                    t / (1024 * 1024)
                ),
                _ => log::info!("  downloaded {} MiB", copied / (1024 * 1024)),
            }
            next_report += 50 * 1024 * 1024;
        }
    }
    Ok(())
}
