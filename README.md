# Wispr

Wispr is a native Ubuntu GNOME dictation tool for Wayland. It runs as a background daemon, captures audio from a selected microphone, streams live audio to Deepgram, and types into the currently focused application through a virtual keyboard. Finalized spoken segments can also be interpreted by a configurable OpenAI-compatible LLM so spoken commands become editing actions instead of literal text.

This repository currently targets Ubuntu GNOME on Wayland.

## Current Behavior

- `wisprd` runs as a user service and exposes a D-Bus control API
- `wispr-settings` is the GTK4/libadwaita settings window
- `wisprctl` is the CLI for setup and daemon control
- microphone selection is stored in `~/.config/wispr/config.toml`
- the Deepgram API key and LLM API key are stored in GNOME Secret Service, not in the config file
- direct typing uses `/dev/uinput`
- live capture currently uses `pw-record` for the audio stream and GStreamer only for device enumeration
- finalized transcript segments can be passed through an OpenAI-compatible `responses` backend for structured command interpretation
- the LLM layer supports literal dictation, editing actions, and literal text plus actions in the same spoken segment

## Capabilities

- live dictation into the focused app
- configurable microphone selection with persistent device choice
- Deepgram speech-to-text streaming
- OpenAI-compatible command interpretation with configurable base URL, model, and API key
- spoken editing commands such as `hello enter`, `select all`, `copy`, `paste`, `undo`, and `redo`
- repeated key actions such as `press space key twice`
- function key actions such as `press the F5 key`
- shell-style text cleanup for command dictation, for example `flutter dash dash version enter` becoming `flutter --version` followed by `Enter`

## Workspace Layout

- `crates/wispr-core`: shared config, models, D-Bus interface, secret storage, typing diffing, and install helpers
- `bins/wisprd`: background daemon, Deepgram streaming client, audio capture, overlay, and shortcut handling
- `bins/wispr-settings`: GTK4/libadwaita settings window
- `bins/wisprctl`: CLI for daemon control, autostart install, default config generation, and `/dev/uinput` setup
- `assets/systemd/wisprd.service`: user service template
- `assets/desktop/wispr-settings.desktop`: desktop launcher
- `scripts/setup-uinput.sh`: helper script for `/dev/uinput` permission setup

## Runtime Requirements

Wispr expects these tools or services to exist at runtime:

- PipeWire with `pw-record`
- GNOME Secret Service
- `systemd --user`
- `/dev/uinput`
- a Deepgram API key
- an OpenAI-compatible LLM API key if intelligence is enabled

## Build Dependencies

Install the native packages Wispr needs on Ubuntu:

```bash
sudo apt-get install -y \
  cargo rustc pkg-config \
  libgtk-4-dev libadwaita-1-dev libgraphene-1.0-dev \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
  libpipewire-0.3-dev
```

## Build

Build the workspace:

```bash
cargo build
```

The main binaries will be:

- `target/debug/wisprd`
- `target/debug/wisprctl`
- `target/debug/wispr-settings`

## Installation

1. Write the default config:

```bash
cargo run --bin wisprctl -- write-default-config
```

2. Install the binaries into `~/.local/bin`:

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

Then:

- store your Deepgram API key
- optionally enable Intelligence and store your LLM API key
- set the LLM base URL and model if you are not using the default OpenAI endpoint
- select the microphone you want to use
- save your settings

## Intelligence Configuration

Wispr can interpret finalized speech through a configurable OpenAI-compatible `responses` API backend. These fields live under `[intelligence]` in `~/.config/wispr/config.toml`:

```toml
[intelligence]
enabled = true
base_url = "https://api.openai.com/v1"
model = "gpt-4o-mini"
timeout_ms = 2500
max_recent_chars = 256
command_mode = "always_infer"
text_output_mode = "literal"
action_scope = "editing_only"
debug_overlay = true
```

The LLM API key is stored separately in GNOME Secret Service. The settings UI includes a `Test LLM` button, and the CLI exposes a direct interpreter test command.

## Day-To-Day Use

Start or stop dictation with:

```bash
~/.local/bin/wisprctl toggle
```

Other useful commands:

```bash
~/.local/bin/wisprctl start
~/.local/bin/wisprctl stop
~/.local/bin/wisprctl status
~/.local/bin/wisprctl open-settings
~/.local/bin/wisprctl test-llm "hello enter"
```

## Intelligent Commands

Examples of phrases that Wispr can now interpret:

- `hello enter` -> types `hello` and presses `Enter`
- `select all` -> sends `Ctrl+A`
- `press space key twice` -> presses `Space` twice
- `press the F5 key` -> presses `F5`
- `flutter dash dash version enter` -> types `flutter --version` and presses `Enter`

The LLM layer is constrained to editing-oriented actions only. It does not launch apps, run shell commands itself, click the mouse, or send arbitrary shortcuts.

## Hotkey Behavior

Wispr first tries to register a Wayland global shortcut through the XDG desktop portal.

On some GNOME sessions that portal registration fails with `org.freedesktop.DBus.Error.NoReply`. In that case, Wispr falls back to a GNOME custom shortcut instead of failing completely.

The fallback shortcut used in this setup is:

- `Windows + Shift + D`

That GNOME shortcut runs:

```bash
~/.local/bin/wisprctl toggle
```

If you want to inspect the configured fallback shortcut:

```bash
gsettings get org.gnome.settings-daemon.plugins.media-keys custom-keybindings
gsettings get org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/custom0/ binding
gsettings get org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/custom0/ command
```

## Files and State

- config: `~/.config/wispr/config.toml`
- user service: `~/.config/systemd/user/wisprd.service`
- desktop entry: `~/.local/share/applications/wispr-settings.desktop`
- binaries: `~/.local/bin/wisprd`, `~/.local/bin/wisprctl`, `~/.local/bin/wispr-settings`

## Troubleshooting

### Daemon status

Check the daemon:

```bash
~/.local/bin/wisprctl status
~/.local/bin/wisprctl test-llm "hello enter"
systemctl --user status wisprd.service --no-pager
journalctl --user -u wisprd.service -n 50 --no-pager
```

The daemon status now includes LLM-related fields such as `intelligence_ready`, `llm_ready`, `last_llm_error`, `last_decision_kind`, and `intelligence_state`.

### No typing

Make sure `/dev/uinput` is usable:

```bash
ls -l /dev/uinput
id
```

Your user should be in the `wisprinput` group after running `setup-uinput` and logging out and back in.

### Microphone debugging

Record from a specific PipeWire source:

```bash
pw-record --target alsa_input.usb-046d_C270_HD_WEBCAM_4F74BC60-02.mono-fallback --rate 16000 --channels 1 --format s16 /tmp/wispr-webcam.wav
pw-play /tmp/wispr-webcam.wav
```

### Hotkey not firing

If the portal shortcut path fails, confirm the GNOME fallback shortcut exists:

```bash
gsettings get org.gnome.settings-daemon.plugins.media-keys custom-keybindings
```

If needed, verify the fallback command executes:

```bash
~/.local/bin/wisprctl toggle
```

### LLM fallback or literal-only behavior

If Wispr keeps typing literal text for commands, check:

```bash
~/.local/bin/wisprctl status
~/.local/bin/wisprctl test-llm "select all"
journalctl --user -u wisprd.service -n 50 --no-pager
```

Common causes:

- no LLM API key stored in Secret Service
- incorrect LLM base URL or model
- backend compatibility issues with the OpenAI-compatible `responses` API
- a backend that rejects strict JSON schema output

## Notes

- the current Deepgram client uses the streaming listen endpoint and `nova-3`
- the current capture path uses `pw-record` because it behaved more reliably on this machine than the earlier GStreamer live capture path
- the LLM interpreter prefers streaming `responses` but falls back to a non-streaming `responses` request when a compatible backend closes the stream noisily
- the daemon can press `Enter` or other supported keys only when the LLM interpreter explicitly returns those actions
