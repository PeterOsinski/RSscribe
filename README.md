# RSscribe

**Speak and it types.** Local, private, offline voice dictation for macOS.

RSscribe (as in *Rust Scribe*) records your microphone, transcribes the audio
**on your own machine** with [Whisper](https://github.com/ggml-org/whisper.cpp),
and pastes the text into whatever you're looking at. No cloud, no API keys, no
subscription. It understands ~99 languages and auto-detects yours.

The command-line binary is named `voice-paste`.

```
double-tap Option  →  mic capture (VAD)  →  whisper-server (Metal)  →  paste
```

## Highlights

- **Fully local** — transcription runs through `whisper-server` (whisper.cpp) on
  your GPU (Metal on Apple Silicon). Nothing leaves your machine.
- **Dictate anywhere** — a menu-bar dot (grey = idle, red = listening) and a
  double-tap of Option toggles continuous dictation into the focused field.
- **Talk naturally** — a voice-activity detector splits your speech on pauses
  and pastes phrase-by-phrase, so you just keep talking.
- **Also a CLI** — record a clip, or transcribe existing audio files
  (WAV/MP3/FLAC/OGG/OPUS natively, M4A/AAC via ffmpeg).
- **Pick your model** — from `tiny` to `large-v3-turbo`, quantized or full, to
  trade speed against accuracy.

## Privacy

Transcription runs entirely on this machine, and that is a claim you can check
rather than trust:

- There is **no account, no API key, no telemetry and no subscription**. Audio is
  captured, transcribed by the local `whisper-server`, and pasted into the focused
  field — it is never uploaded anywhere.
- The **only network request** the tool makes is the one-time model download from
  `huggingface.co/ggerganov/whisper.cpp` — a public URL that needs no credentials —
  cached under `~/Library/Caches/voice-paste/`.
- **To verify it yourself:** dictate with Wi-Fi turned off (it keeps working), or
  run the daemon with Little Snitch or `lsof -i` open and confirm no outbound
  connection appears while it transcribes. Deletion is a local file operation here,
  not a promise about someone else's retention policy.

Dictation tends to capture things that never get typed — names, numbers, notes
about other people. Voice is also biometric data, which is why on-device
processing matters more here than in most apps. If you need the biggest hosted
models, a cloud dictation product will beat this one on accuracy; that is a real
trade-off, and the point of this tool is to be the option that does not make it.

## Install

Prerequisites: **macOS 14+ on Apple Silicon** (primary target) and
[Homebrew](https://brew.sh). Linux works for the CLI/transcription; the hotkey
and paste paths are macOS-first.

```bash
# 1. Install Rust (skip if you already have it)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 2. Install whisper.cpp (provides whisper-server) and ffmpeg
brew install whisper.cpp ffmpeg

# 3. Build
cargo build --release

# 4. (optional) put the binary on your PATH
cp target/release/voice-paste /opt/homebrew/bin/
```

That's it. The Whisper model (~1.5 GB for the default `large-v3-turbo`)
downloads automatically on first use and is cached under
`~/Library/Caches/voice-paste/`.

### macOS permissions

On first run, grant the permissions macOS asks for in
**System Settings → Privacy & Security**:

- **Microphone** — to capture audio.
- **Accessibility** — to simulate the paste keystroke.
- **Input Monitoring** — to see the global Option hotkey (daemon mode only).

## Run it

```bash
voice-paste                     # daemon: double-tap Option to dictate continuously
voice-paste record              # record once, print the transcript
voice-paste record --paste      # record once and paste into the focused app
voice-paste transcribe interview.m4a   # transcribe one or more audio files
voice-paste setup               # download the model and exit
voice-paste stop                # stop the resident server and free its memory
```

Useful flags: `--lang en|pl|auto`, `--model <name>`, `--model-dir <path>`,
`--bin-dir <path>`, `--paste`.

### Daemon mode (the main event)

Run `voice-paste` and a dot appears in the macOS menu bar. **Double-tap Option**
to start dictating — speak naturally and each phrase is pasted into the focused
input as you pause (~0.7 s of silence). Double-tap Option again to stop. The dot
turns grey (idle) ↔ red (recording) so you can see the state at a glance.

It uses a double-tap rather than a chord (e.g. Option+Space) because the hotkey
layer *observes* global keys but can't swallow them: a bare Option press leaks
nothing into your app, while Option+Space would type a non-breaking space.

## Models

| Model | Size | Notes |
|-------|------|-------|
| `large-v3-turbo` *(default)* | 1.5 GB | best accuracy for dictation; multilingual |
| `large-v3-turbo-q5_0` | 547 MB | quantized; smaller/faster load, similar quality |
| `medium` | 1.5 GB | good multilingual accuracy |
| `small` | 466 MB | ~4× faster, near-instant, small accuracy cost |
| `tiny` / `base` | 75–142 MB | fastest, lower accuracy |

Choose with `--model <name>` or `VOICE_PASTE_MODEL`. More names are supported —
`tiny`, `base`, `small`, `medium`, `large-v1/v2/v3`, plus `.en` and quantized
(`-q5_0`/`-q8_0`) variants.

## Configuration

| Env var | Default | Purpose |
|---------|---------|---------|
| `VOICE_PASTE_CACHE_DIR` | `~/Library/Caches/voice-paste` | cache root |
| `VOICE_PASTE_MODEL_DIR` | `<cache>/models/whisper` | model directory |
| `VOICE_PASTE_BIN_DIR` | auto-detected | directory containing `whisper-server` |
| `VOICE_PASTE_LANG` | `auto` | default language (`en`, `pl`, `auto`, …) |
| `VOICE_PASTE_MODEL` | `large-v3-turbo` | default model |
| `VOICE_PASTE_PORT` | `8080` | local server port |
| `VOICE_PASTE_AUDIO_CTX` | `0` | experimental: cap encoder context (smaller = faster, but can duplicate text) |

## Project layout

```
src/main.rs       CLI parsing + daemon hotkey loop
src/audio.rs      mic capture → mono WAV, VAD/RMS
src/transcribe.rs resident whisper-server client + lifecycle
src/paste.rs      clipboard set + simulated paste
src/assets.rs     server discovery + model auto-download
src/text.rs       transcript cleanup
src/status.rs     macOS menu-bar status dot (grey idle / red recording)
site/index.html   minimal landing page (no framework)
```

## Licence

MIT — use it, fork it, ship it, sell it. Provided **as is, without warranty of any
kind**. See [LICENSE](LICENSE) for the full text.

## Contributing

Issues and pull requests are welcome. The tool targets **macOS 14+ on Apple
Silicon** first: the CLI and transcription paths are portable, while the hotkey and
paste paths are macOS-specific. Please do not commit `.env` files, credentials or
`.DS_Store` — the `.gitignore` covers them, and history is forever.
