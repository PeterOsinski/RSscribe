# voice-paste

Record audio, transcribe it locally with
[Whisper](https://github.com/ggml-org/whisper.cpp), and optionally paste the
result into whatever is focused.

It runs two ways:

1. **CLI** — `voice-paste record` records from the mic and prints the transcript.
2. **Background daemon** — a global hotkey toggles recording and pastes the
   transcript into the focused input.

The Whisper model is downloaded automatically on first use. It is multilingual,
so English and Polish (and ~97 other languages) work out of the box with
auto-detection.

## How it works

Transcription is handled by `whisper-server` from
[whisper.cpp](https://github.com/ggml-org/whisper.cpp), run as a local HTTP
server. It is started on first use and kept **resident** — the model is loaded
once and reused, so warm transcriptions take ~1–2 s instead of reloading the
model every time. On Apple Silicon it uses the GPU (Metal) automatically.

## Requirements

- **Rust** (stable) — `cargo build --release`
- **whisper.cpp** installed, which provides the `whisper-server` binary:

  ```bash
  brew install whisper.cpp
  ```

  (Or build whisper.cpp from source and put `whisper-server` on your `PATH`, or
  point `VOICE_PASTE_BIN_DIR` at a directory containing it.)
- **ffmpeg** (only needed to transcribe non-native formats like `.m4a`/AAC via
  `voice-paste transcribe file.m4a`; mic recording writes WAV and needs no
  ffmpeg): `brew install ffmpeg`
- **macOS 14+ Apple Silicon** (or Linux) — `whisper-server` runs anywhere, but
  this app's paste/hotkey paths are best supported on macOS.

No API keys or tokens are required. The model is downloaded from the public
[`ggerganov/whisper.cpp`](https://huggingface.co/ggerganov/whisper.cpp)
HuggingFace repo.

## Build

```bash
cargo build --release
```

## Usage

```bash
# record from the microphone, print the transcript (auto-detects language)
./target/release/voice-paste record

# record and paste into the focused app
./target/release/voice-paste record --paste

# transcribe an existing audio file (WAV/MP3/FLAC/OGG/OPUS natively;
# m4a/AAC converted via ffmpeg)
./target/release/voice-paste transcribe interview.m4a

# force a language (instead of auto-detect)
./target/release/voice-paste record --lang en
./target/release/voice-paste record --lang pl

# use a different Whisper model (default: large-v3-turbo)
./target/release/voice-paste record --model medium

# just download the model and exit
./target/release/voice-paste setup

# stop the resident server and free its memory
./target/release/voice-paste stop

# run in the background and toggle recording with a hotkey
./target/release/voice-paste
```

On first run the app downloads the Whisper model (~1.5 GB for the default
`large-v3-turbo`) into the cache directory (see below). This happens once.

The transcription server is started automatically on first use and left running
so subsequent runs are fast. Stop it with `voice-paste stop` (or
`pkill whisper-server`) when you want the memory back.

## Hotkey (daemon mode)

Default is **Ctrl+Alt+Space** (Cmd+Alt+Space on macOS). First press starts
recording, second press stops, transcribes, and pastes. Change it in
`is_trigger_combo` in `src/main.rs`.

## Models

| Model | Size | Notes |
|-------|------|-------|
| `large-v3-turbo` *(default)* | 1.5 GB | Best speed/accuracy for dictation; multilingual |
| `large-v3-turbo-q5_0` | 547 MB | Quantized, smaller/faster, near-identical quality |
| `medium` | 1.5 GB | Good multilingual accuracy |
| `small` | 466 MB | Fast, lower accuracy |

Choose with `--model <name>` or `VOICE_PASTE_MODEL`.

## Configuration

| Env var | Default | Purpose |
|---------|---------|---------|
| `VOICE_PASTE_CACHE_DIR` | `~/Library/Caches/voice-paste` (macOS) / `~/.cache/voice-paste` (Linux) | Cache root |
| `VOICE_PASTE_MODEL_DIR` | `<cache>/models/whisper` | Model directory |
| `VOICE_PASTE_BIN_DIR` | *(auto-detected)* | Directory holding `whisper-server` |
| `VOICE_PASTE_LANG` | `auto` | Default language (`en`, `pl`, `auto`, …) |
| `VOICE_PASTE_MODEL` | `large-v3-turbo` | Default model |
| `VOICE_PASTE_PORT` | `8080` | Port for the local server |

## Platform notes

- **macOS**: the daemon needs *Input Monitoring* (global key events),
  *Accessibility* (simulated paste keystroke), and *Microphone* (capture)
  permissions. The CLI `record` mode only needs *Microphone*.
- **Linux**: `rdev` and `enigo` synthetic input are reliable on X11; Wayland
  compositors vary.

## Project layout

```
src/main.rs      CLI parsing + daemon hotkey loop
src/audio.rs     cpal mic capture → mono WAV
src/transcribe.rs  resident whisper-server client + lifecycle
src/paste.rs     clipboard set + simulated paste
src/assets.rs    whisper-server discovery + model auto-download
src/text.rs      transcript cleanup (whitespace + repetition normalization)
```
