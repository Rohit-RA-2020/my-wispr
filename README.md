# Wispr

Wispr is a native Ubuntu GNOME dictation tool for Wayland. It captures audio from a user-selected microphone, streams live audio to Deepgram Flux, and types the transcript into the currently focused application through a virtual keyboard.

## Workspace Layout

- `crates/wispr-core`: shared config, D-Bus interface, secret storage, typing diffing, and install helpers
- `bins/wisprd`: background daemon with D-Bus service, Deepgram streaming, GStreamer/PipeWire capture, overlay, and portal shortcut handling
- `bins/wispr-settings`: GTK4/libadwaita settings window for onboarding and diagnostics
- `bins/wisprctl`: CLI for daemon control, autostart install, and `/dev/uinput` setup

## System Packages

This repo expects these native packages on Ubuntu:

```bash
sudo apt-get install -y \
  cargo rustc pkg-config \
  libgtk-4-dev libadwaita-1-dev libgraphene-1.0-dev \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
  libpipewire-0.3-dev
```

## First Run

1. Build the binaries:

```bash
cargo build
```

2. Create the default config:

```bash
wisprctl write-default-config
```

3. Install the binaries and autostart service:

```bash
mkdir -p ~/.local/bin ~/.local/share/applications
cp target/debug/wisprd target/debug/wisprctl target/debug/wispr-settings ~/.local/bin/
cp assets/desktop/wispr-settings.desktop ~/.local/share/applications/
wisprctl install-autostart
```

4. Run the one-time uinput setup:

```bash
sudo wisprctl setup-uinput
```

5. Log out and back in so your user picks up the `wisprinput` group.

6. Start the daemon:

```bash
systemctl --user daemon-reload
systemctl --user enable --now wisprd.service
```

7. Open the settings UI and store your Deepgram API key:

```bash
wispr-settings
```

## Autostart

Generate a user service file with:

```bash
wisprctl install-autostart
```

Then enable it:

```bash
systemctl --user daemon-reload
systemctl --user enable --now wisprd.service
```
