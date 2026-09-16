mod assets;
mod audio;
mod paste;
mod text;
mod transcribe;

use anyhow::{bail, Result};
use rdev::{listen, Event, EventType, Key};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

const USAGE: &str = "\
voice-paste — record audio, transcribe locally with Whisper, and (optionally) paste it.

USAGE:
    voice-paste                 Run in the background and toggle recording with a hotkey
    voice-paste record          Record from the microphone, transcribe, and print the text
    voice-paste transcribe <FILE>...  Transcribe one or more existing audio files
    voice-paste setup           Download the Whisper model and exit
    voice-paste stop            Stop the resident whisper-server and free its memory

OPTIONS:
    --lang <CODE>               Language code: 'en', 'pl', or 'auto' (default: auto)
    --model <NAME>              Whisper model (default: large-v3-turbo)
    --model-dir <PATH>          Model directory (default: auto-managed cache dir)
    --bin-dir <PATH>            Directory holding the whisper-server binary
    --paste                     Paste the transcript into the focused app (record/transcribe)
    -h, --help                  Show this help

ENV:
    VOICE_PASTE_MODEL_DIR       Override the model directory
    VOICE_PASTE_BIN_DIR         Override the whisper-server binary directory
    VOICE_PASTE_CACHE_DIR       Override the cache root
    VOICE_PASTE_LANG            Default language (default: auto)
    VOICE_PASTE_MODEL           Default model (default: large-v3-turbo)
    VOICE_PASTE_PORT            Port for the local server (default: 8080)

Hotkey (daemon mode): Ctrl+Alt+Space (Cmd+Alt+Space on macOS). First press starts
recording, second press stops, transcribes, and pastes.
";

struct Cli {
    command: Command,
    files: Vec<PathBuf>,
    lang: String,
    model: String,
    model_dir: Option<PathBuf>,
    bin_dir: Option<PathBuf>,
    paste: bool,
}

enum Command {
    Daemon,
    Record,
    Transcribe,
    Setup,
    Stop,
    Help,
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = parse_args()?;

    if matches!(cli.command, Command::Help) {
        print!("{USAGE}");
        return Ok(());
    }

    let (default_bin_dir, default_model_dir) = assets::default_paths();
    let model_dir = cli.model_dir.as_deref().unwrap_or(&default_model_dir);
    let _bin_dir = cli.bin_dir.as_deref().unwrap_or(&default_bin_dir);

    match cli.command {
        Command::Setup => {
            let assets = assets::ensure(model_dir, &cli.model)?;
            log::info!(
                "assets ready: whisper-server at {}, model at {}",
                assets.server_bin.display(),
                assets.model_path.display()
            );
            Ok(())
        }
        Command::Stop => {
            stop_server();
            Ok(())
        }
        Command::Record => {
            let assets = assets::ensure(model_dir, &cli.model)?;
            record_and_transcribe(&assets, &cli.lang, cli.paste)
        }
        Command::Transcribe => {
            let assets = assets::ensure(model_dir, &cli.model)?;
            transcribe_files(&assets, &cli.lang, &cli.files, cli.paste)
        }
        Command::Daemon => {
            let assets = assets::ensure(model_dir, &cli.model)?;
            run_daemon(&assets, &cli.lang)
        }
        Command::Help => unreachable!(),
    }
}

/// Kill the resident whisper-server (and free its memory).
fn stop_server() {
    match std::process::Command::new("pkill").arg("-f").arg("whisper-server").status() {
        Ok(status) if status.success() => log::info!("stopped whisper-server"),
        Ok(_) => log::info!("no running whisper-server found"),
        Err(e) => log::warn!("failed to stop server: {e}"),
    }
}

fn parse_args() -> Result<Cli> {
    let mut command = Command::Daemon;
    let mut files: Vec<PathBuf> = Vec::new();
    let mut lang = std::env::var("VOICE_PASTE_LANG").unwrap_or_else(|_| "auto".into());
    let mut model = std::env::var("VOICE_PASTE_MODEL").unwrap_or_else(|_| assets::DEFAULT_MODEL.into());
    let mut model_dir = None;
    let mut bin_dir = None;
    let mut paste = false;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-h" | "--help" => command = Command::Help,
            "--paste" => paste = true,
            "--lang" | "--language" => {
                i += 1;
                lang = args
                    .get(i)
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("missing value for {arg}"))?;
            }
            "--model" => {
                i += 1;
                model = args
                    .get(i)
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("missing value for {arg}"))?;
            }
            "--model-dir" => {
                i += 1;
                model_dir = Some(PathBuf::from(
                    args.get(i)
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!("missing value for {arg}"))?,
                ));
            }
            "--bin-dir" => {
                i += 1;
                bin_dir = Some(PathBuf::from(
                    args.get(i)
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!("missing value for {arg}"))?,
                ));
            }
            "record" => command = Command::Record,
            "setup" => command = Command::Setup,
            "stop" => command = Command::Stop,
            "transcribe" => command = Command::Transcribe,
            "help" => command = Command::Help,
            other if other.starts_with('-') => bail!("unknown option: {other}\n\n{USAGE}"),
            other => files.push(PathBuf::from(other)),
        }
        i += 1;
    }

    if matches!(command, Command::Transcribe) && files.is_empty() {
        bail!("transcribe requires at least one audio file\n\n{USAGE}");
    }

    Ok(Cli { command, files, lang, model, model_dir, bin_dir, paste })
}

// ---------------------------------------------------------------------------
// CLI: one-shot record / transcribe
// ---------------------------------------------------------------------------

fn record_and_transcribe(assets: &assets::Assets, lang: &str, paste: bool) -> Result<()> {
    // Start (or reuse) the resident server first so the model is loaded before
    // we start recording — repeat runs are then near-instant.
    let client = transcribe::ensure_server_running(
        &assets.server_bin,
        &assets.model_path,
        transcribe::default_port(),
    )?;

    log::info!("starting microphone capture — press Enter to stop recording");
    let rec = audio::start_recording()?;

    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;

    let (samples, rate) = audio::stop_recording(rec)?;
    let text = transcribe_samples(&client, &samples, rate, lang)?;

    if text.trim().is_empty() {
        log::warn!("empty transcript");
        return Ok(());
    }

    if paste {
        paste::paste_text(&text)?;
    }
    println!("{text}");
    Ok(())
}

/// Write the recorded samples to a WAV file and transcribe them. whisper
/// handles long recordings natively (VAD + sentence segmentation), so the
/// whole clip goes through in one request.
fn transcribe_samples(
    client: &transcribe::ServerClient,
    samples: &[f32],
    sample_rate: u32,
    lang: &str,
) -> Result<String> {
    let wav_path = std::env::temp_dir().join("voice-paste-recording.wav");
    audio::write_wav(samples, sample_rate, &wav_path)?;
    let result = client.transcribe(&wav_path, lang);
    let _ = std::fs::remove_file(&wav_path);
    result
}

fn transcribe_files(assets: &assets::Assets, lang: &str, files: &[PathBuf], paste: bool) -> Result<()> {
    let client = transcribe::ensure_server_running(
        &assets.server_bin,
        &assets.model_path,
        transcribe::default_port(),
    )?;

    let mut combined = Vec::new();
    for file in files {
        if files.len() > 1 {
            println!("[{}]", file.display());
        }

        let text = transcribe_one(&client, file, lang)?;

        if text.trim().is_empty() {
            log::warn!("empty transcript for {}", file.display());
        } else {
            let t = text.trim().to_string();
            combined.push(t.clone());
            println!("{t}");
        }
    }

    if paste && !combined.is_empty() {
        paste::paste_text(&combined.join("\n"))?;
    }
    Ok(())
}

/// Transcribe a single file. Formats whisper-server decodes natively are sent
/// as-is; everything else (e.g. m4a/AAC) is converted to WAV with ffmpeg first.
fn transcribe_one(client: &transcribe::ServerClient, file: &Path, lang: &str) -> Result<String> {
    if is_native_audio(file) {
        return client.transcribe(file, lang);
    }

    let tmp = std::env::temp_dir().join("voice-paste-converted.wav");
    let status = std::process::Command::new("ffmpeg")
        .args(["-y", "-loglevel", "error", "-i"])
        .arg(file)
        .args(["-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le"])
        .arg(&tmp)
        .status();

    match status {
        Ok(s) if s.success() => {
            let result = client.transcribe(&tmp, lang);
            let _ = std::fs::remove_file(&tmp);
            result
        }
        _ => {
            // ffmpeg missing or conversion failed — try sending the file raw.
            client.transcribe(file, lang)
        }
    }
}

fn is_native_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            matches!(
                e.to_ascii_lowercase().as_str(),
                "wav" | "mp3" | "flac" | "ogg" | "opus" | "wma"
            )
        })
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Daemon: background hotkey loop
// ---------------------------------------------------------------------------

// Ctrl+Alt+Space (Cmd+Alt+Space on macOS, where Meta maps to Cmd).
fn is_trigger_combo(held: &HashSet<Key>) -> bool {
    let ctrl_or_cmd = held.contains(&Key::ControlLeft)
        || held.contains(&Key::ControlRight)
        || held.contains(&Key::MetaLeft)
        || held.contains(&Key::MetaRight);
    let alt = held.contains(&Key::Alt) || held.contains(&Key::AltGr);
    let space = held.contains(&Key::Space);
    ctrl_or_cmd && alt && space
}

enum AppEvent {
    HotkeyPressed,
}

fn run_daemon(assets: &assets::Assets, lang: &str) -> Result<()> {
    let server = transcribe::ensure_server_running(
        &assets.server_bin,
        &assets.model_path,
        transcribe::default_port(),
    )?;
    log::info!("ready — press the hotkey to toggle recording");

    let lang = lang.to_string();
    let recording_state: Arc<Mutex<Option<audio::Recording>>> = Arc::new(Mutex::new(None));

    let (tx, rx) = channel::<AppEvent>();
    spawn_hotkey_listener(tx);

    for event in rx {
        if !matches!(event, AppEvent::HotkeyPressed) {
            continue;
        }
        let mut state = recording_state.lock().unwrap();
        if state.is_none() {
            log::info!("recording started");
            match audio::start_recording() {
                Ok(rec) => *state = Some(rec),
                Err(e) => log::error!("failed to start recording: {e}"),
            }
        } else {
            log::info!("recording stopped, transcribing...");
            let rec = state.take().unwrap();
            drop(state);

            let (samples, rate) = match audio::stop_recording(rec) {
                Ok(s) => s,
                Err(e) => {
                    log::error!("failed to stop recording: {e}");
                    continue;
                }
            };

            match transcribe_samples(&server, &samples, rate, &lang) {
                Ok(text) if !text.trim().is_empty() => {
                    log::info!("transcript: {text}");
                    if let Err(e) = paste::paste_text(&text) {
                        log::error!("failed to paste: {e}");
                    }
                }
                Ok(_) => log::warn!("empty transcript, nothing to paste"),
                Err(e) => log::error!("transcription failed: {e}"),
            }
        }
    }

    Ok(())
}

fn spawn_hotkey_listener(tx: Sender<AppEvent>) {
    thread::spawn(move || {
        let held: Arc<Mutex<HashSet<Key>>> = Arc::new(Mutex::new(HashSet::new()));
        let already_fired = Arc::new(Mutex::new(false));

        let held_cb = held.clone();
        let fired_cb = already_fired.clone();

        let callback = move |event: Event| {
            let mut held = held_cb.lock().unwrap();
            match event.event_type {
                EventType::KeyPress(key) => {
                    held.insert(key);
                    let mut fired = fired_cb.lock().unwrap();
                    if !*fired && is_trigger_combo(&held) {
                        *fired = true;
                        let _ = tx.send(AppEvent::HotkeyPressed);
                    }
                }
                EventType::KeyRelease(key) => {
                    held.remove(&key);
                    if !is_trigger_combo(&held) {
                        *fired_cb.lock().unwrap() = false;
                    }
                }
                _ => {}
            }
        };

        if let Err(e) = listen(callback) {
            log::error!("hotkey listener failed: {e:?}");
        }
    });
}
