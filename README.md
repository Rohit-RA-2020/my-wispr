[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/Rohit-RA-2020/my-wispr)

# Wispr

Wispr is a desktop dictation tool with first-class support for both Linux and macOS.
It runs a background Rust daemon, captures microphone audio, transcribes speech through either Deepgram or local Whisper, and types into the focused application. Finalized transcript segments can also be interpreted by an OpenAI-compatible LLM so spoken commands become editing actions, semantic shortcuts, formatted text, or autonomous generated writing instead of literal text.

This repository now supports two primary desktop targets:

- Linux: GTK settings app, systemd user service, `/dev/uinput` typing, PipeWire capture
- macOS: SwiftUI menu bar app, LaunchAgent autostart, Apple automation typing, AVFoundation capture through `ffmpeg`

## Platform Summary

| Area | Linux | macOS |
| --- | --- | --- |
| Main control surface | `wispr-settings` GTK app + `wisprctl` | `Wispr.app` menu bar app + `wisprctl` |
| Background service | `wisprd` | `wisprd` |
| IPC | Unix socket JSON API | Unix socket JSON API |
| Config path | `~/.config/wispr/config.toml` | `~/Library/Application Support/wispr/config.toml` |
| Socket path | `~/.config/wispr/wisprd.sock` | `~/Library/Application Support/wispr/wisprd.sock` |
| Secret storage | GNOME Secret Service | macOS Keychain |
| Autostart | `systemd --user` | LaunchAgent |
| Audio capture | PipeWire / `pw-record` | `ffmpeg -f avfoundation` |
| Typing backend | `/dev/uinput` | `osascript` / System Events |
| Native UI | GTK4/libadwaita | SwiftUI menu bar app |

## Current Capabilities

Shared capabilities across both platforms:

- live dictation into the focused app
- Deepgram streaming speech-to-text
- local Whisper speech-to-text with downloadable models
- OpenAI-compatible command interpretation with configurable base URL and model
- spoken editing commands such as `hello enter`, `copy`, `paste`, `undo`, and `redo`
- dynamic spoken shortcuts such as `press control t` and `press control shift p`
- semantic commands such as `open a new browser tab`, `save file`, and `refresh this page`
- structured text formatting for lists and rewritten blocks
- autonomous writing mode for explicit generation requests
- CLI control through `wisprctl`

Platform-specific behavior:

- Linux uses GNOME-aware active-app detection and a GTK settings window.
- macOS uses a SwiftUI menu bar app, a built-in global shortcut, and system privacy panes for permissions.

## Workspace Layout

- `crates/wispr-core`: shared config, models, IPC client, secret storage, typing diffing, shortcuts, install helpers
- `bins/wisprd`: background daemon, transcription backends, audio capture, overlay, status handling
- `bins/wisprctl`: CLI for daemon control, setup, config updates, diagnostics, and install helpers
- `bins/wispr-settings`: Linux GTK settings app
- `apps/WisprMac`: macOS SwiftUI menu bar app
- `scripts/install_wispr_mac_dev.sh`: local macOS app bundle installer for development
- `assets/systemd/wisprd.service`: Linux user-service template
- `assets/desktop/wispr-settings.desktop`: Linux desktop launcher

## Runtime Requirements

Common:

- Rust toolchain
- a Deepgram API key for cloud transcription
- `python3` and `ffmpeg` for local Whisper transcription
- an OpenAI-compatible API key if intelligence is enabled

Linux:

- PipeWire with `pw-record`
- GNOME Secret Service
- `systemd --user`
- `/dev/uinput`
- GTK4/libadwaita runtime for `wispr-settings`

macOS:

- macOS 13+
- `ffmpeg` available at runtime, typically from Homebrew
- Accessibility permission for text injection
- Microphone permission for audio capture
- Input Monitoring may be needed depending on your automation/security settings

## Build Dependencies

### Linux

Install the native packages Wispr needs on Ubuntu or another Debian-based distro:

```bash
sudo apt-get install -y \
  cargo rustc pkg-config python3 ffmpeg \
  libgtk-4-dev libadwaita-1-dev libgraphene-1.0-dev \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
  libpipewire-0.3-dev
```

### macOS

Install the core dependencies:

```bash
brew install ffmpeg
```

You also need:

- Rust toolchain
- Xcode Command Line Tools / Swift 5.9+ to build `apps/WisprMac`

## Build

Build the Rust workspace:

```bash
cargo build
```

Main binaries:

- `target/debug/wisprd`
- `target/debug/wisprctl`
- `target/debug/wispr-settings` on Linux

Build the macOS app shell:

```bash
cd apps/WisprMac
swift build
```

## Installation

### Linux Installation

1. Write the default config:

```bash
cargo run --bin wisprctl -- write-default-config
```

2. Install binaries:

```bash
mkdir -p ~/.local/bin
install -m 0755 target/debug/wisprd ~/.local/bin/wisprd
install -m 0755 target/debug/wisprctl ~/.local/bin/wisprctl
install -m 0755 target/debug/wispr-settings ~/.local/bin/wispr-settings
```

3. Install the desktop entry:

```bash
mkdir -p ~/.local/share/applications
install -m 0644 assets/desktop/wispr-settings.desktop ~/.local/share/applications/wispr-settings.desktop
```

4. Install the user service:

```bash
~/.local/bin/wisprctl install-autostart
systemctl --user daemon-reload
systemctl --user enable --now wisprd.service
```

5. Install `/dev/uinput` permissions:

```bash
sudo ~/.local/bin/wisprctl setup-uinput
```

6. Log out and back in so your user picks up the `wisprinput` group.

7. Open the settings UI:

```bash
~/.local/bin/wispr-settings
```

### macOS Installation

For local development, install the bundled app with:

```bash
./scripts/install_wispr_mac_dev.sh
```

That script builds:

- `wisprd`
- `wisprctl`
- `WisprMacApp`

and installs a local app bundle at:

- `~/Applications/Wispr.app`

Then open it:

```bash
open ~/Applications/Wispr.app
```

The app bundle contains:

- `WisprMacApp`
- `wisprd`
- `wisprctl`

## First-Time Setup

### Linux

Open `wispr-settings` and configure:

- transcription provider: `Cloud (Deepgram)` or `Local (Whisper)`
- Deepgram API key if using cloud transcription
- Whisper runtime and model if using local transcription
- LLM API key, base URL, and model if intelligence is enabled
- microphone selection

### macOS

Open `Wispr.app`, then use the Settings window to configure:

- transcription provider: `Cloud (Deepgram)` or `Local (Whisper)`
- Deepgram API key
- LLM API key
- LLM base URL and model
- Whisper runtime install, model download, and model test
- daemon start/stop and LaunchAgent install
- system permission shortcuts

Current macOS default global shortcut:

- `Control + Option + Space`

Current macOS limitation:

- there is not yet a native microphone picker in the Settings window; the daemon resolves the configured or first available device automatically

## Config and Data Locations

### Linux

- config: `~/.config/wispr/config.toml`
- daemon socket: `~/.config/wispr/wisprd.sock`
- local Whisper data: `~/.local/share/wispr/whisper`
- Whisper virtualenv: `~/.local/share/wispr/whisper-venv`

### macOS

- config: `~/Library/Application Support/wispr/config.toml`
- daemon socket: `~/Library/Application Support/wispr/wisprd.sock`
- local Whisper data: `~/Library/Application Support/Wispr/whisper`
- Whisper virtualenv: `~/Library/Application Support/Wispr/whisper-venv`
- local daemon log from `Wispr.app`: `~/.wispr/logs/wisprd.log`
- LaunchAgent plist: `~/Library/LaunchAgents/io.wispr.wisprd.plist`

## Day-To-Day Use

Common CLI commands:

```bash
wisprctl toggle
wisprctl start
wisprctl stop
wisprctl status
wisprctl doctor
wisprctl open-settings
wisprctl show-config
```

Configuration commands:

```bash
wisprctl set-deepgram-key "<DEEPGRAM_KEY>"
wisprctl set-llm-key "<LLM_KEY>"
wisprctl set-llm-base-url "https://api.openai.com/v1"
wisprctl set-llm-model "gpt-4o-mini"
wisprctl set-provider deepgram      # or: whisper_local
wisprctl set-whisper-model base.en
wisprctl whisper-status
wisprctl install-whisper-runtime
wisprctl download-whisper-model base.en
wisprctl delete-whisper-model base.en
wisprctl test-whisper-model base.en
```

LLM interpreter test commands:

```bash
wisprctl test-llm "hello enter"
wisprctl test-llm "press control t"
wisprctl test-llm --app-class browser "open a new browser tab"
wisprctl test-llm "write an essay on world war two"
```

Platform-specific usage:

- Linux: use the GTK settings app and your Linux autostart/service setup.
- macOS: use the menu bar app for regular control and settings.

## Intelligence Configuration

These fields live under `[intelligence]` in the config file:

```toml
[intelligence]
enabled = true
base_url = "https://api.openai.com/v1"
model = "gpt-4o-mini"
timeout_ms = 2500
generation_timeout_ms = 120000
max_recent_chars = 256
command_mode = "always_infer"
text_output_mode = "literal"
action_scope = "editing_only"
debug_overlay = true
dynamic_shortcuts_enabled = true
semantic_commands_enabled = true
generation_enabled = true
generation_trigger_mode = "explicit_requests"
generation_insert_mode = "replace_request"
generation_target_scope = "any_text_field"
shortcut_denylist_profile = "minimal"
shortcut_allowlist = []
shortcut_denylist = []
```

`timeout_ms` is the short command-interpretation budget. `generation_timeout_ms` is the longer ceiling for autonomous writing streams.

## Intelligent Command Examples

Examples of phrases Wispr can interpret:

- `hello enter`
- `select all`
- `press control t`
- `press control shift p`
- `press alt tab`
- `press super left`
- `press space key twice`
- `press the F5 key`
- `flutter dash dash version enter`
- `open a new browser tab`
- `close this tab`
- `reopen the last closed tab`
- `focus the address bar`
- `save file`

## Autonomous Writing Examples

Examples:

- `write an essay on world war two`
- `draft an email for leave`
- `compose a short reply saying I will join after lunch`

Behavior:

- the spoken request is removed from the field once generation starts
- generated text is streamed into the focused field
- stopping dictation stops generation
- partial generated output remains in the field if stopped mid-stream

## Current Limitations

Linux:

- Linux-first integrations still assume GNOME/Wayland and `/dev/uinput`
- `wispr-settings` is Linux-only

macOS:

- secure text fields such as password boxes are OS-protected and cannot be typed into
- typing currently goes through AppleScript / System Events, so responsiveness is still behind the Linux virtual-keyboard path
- the native Settings window does not yet expose a microphone picker
- local Whisper still depends on Python + `openai-whisper`, not `whisper.cpp`

## Troubleshooting

### Linux

- check daemon status with `wisprctl doctor` and `wisprctl status`
- verify `systemctl --user status wisprd.service`
- if typing fails, confirm `/dev/uinput` permissions and that your user is in `wisprinput`
- if secrets fail, confirm GNOME Secret Service is running

### macOS

- use the installed app at `~/Applications/Wispr.app`, not just `swift run`
- if dictation does not start, check `wisprctl doctor`
- if the app starts but audio does not begin, inspect `~/.wispr/logs/wisprd.log`
- if microphone access was granted to Terminal but not `Wispr`, reinstall the app and launch the bundle directly
- if you installed an older LaunchAgent, remove and reinstall it from the app Settings window
- if `ffmpeg` came from Homebrew, make sure it exists at `/opt/homebrew/bin/ffmpeg` or `/usr/local/bin/ffmpeg`
