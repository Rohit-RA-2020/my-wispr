# Wispr

Wispr is a native Ubuntu GNOME dictation tool for Wayland. It runs as a background daemon, captures audio from a selected microphone, streams live audio to Deepgram, and types the transcript into the currently focused application through a virtual keyboard.

This repository currently targets Ubuntu GNOME on Wayland.

## Current Behavior

- `wisprd` runs as a user service and exposes a D-Bus control API
- `wispr-settings` is the GTK4/libadwaita settings window
- `wisprctl` is the CLI for setup and daemon control
- microphone selection is stored in `~/.config/wispr/config.toml`
- the Deepgram API key is stored in GNOME Secret Service, not in the config file
- direct typing uses `/dev/uinput`
- live capture currently uses `pw-record` for the audio stream and GStreamer only for device enumeration

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
- select the microphone you want to use
- save your settings

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
```

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
systemctl --user status wisprd.service --no-pager
journalctl --user -u wisprd.service -n 50 --no-pager
```

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

## Notes

- the current Deepgram client uses the streaming listen endpoint and `nova-3`
- the current capture path uses `pw-record` because it behaved more reliably on this machine than the earlier GStreamer live capture path
- the daemon never presses `Enter` automatically
