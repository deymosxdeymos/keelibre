# keebyd

Mechanical keyboard sounds for Linux, implemented in Rust. Keebyd reads keyboard events from
evdev, mixes switch recordings at 44.1 kHz, and sends stereo audio through CPAL. A native WebKit
window controls the active switch, tone, volume, spatial placement, and per-key response.

```text
keyboard -> evdev -> keebyd -> CPAL -> PipeWire or ALSA -> audio output
                         |
                         +-> native control panel
```

## Install

You need a Rust toolchain, ALSA development headers, GTK 3, and WebKitGTK 4.1. On Debian or Ubuntu:

```sh
sudo apt install cargo libasound2-dev libgtk-3-dev libwebkit2gtk-4.1-dev
bash tools/install.sh
```

The installer builds a locked release, copies `keebyd` to `~/.local/bin`, and enables the systemd
user service and adds Keebyd to the application menu. Your user must be allowed to read
`/dev/input/event*`. Most distributions grant this
through the `input` group:

```sh
sudo usermod -aG input "$USER"
```

Log out and back in after changing groups. Launch **Keebyd** from the application menu once the
service starts.

## Sound packs

Keebyd loads packs from `~/.local/share/keebyd/sounds` by default. A pack is a directory containing
files named `<group>_<phase>_<variation>.wav`, for example `alpha_down_01.wav`. Supported groups are
`alpha`, `space`, `enter`, `backspace`, `modifier`, `tab`, `arrow`, and `mouse`.

An optional `profile.conf` sets pack loudness:

```ini
normalization_gain = 1.0
```

Generate the included synthetic packs with:

```sh
python3 tools/synth_profiles.py ~/.local/share/keebyd/sounds
```

`tools/import-keeby.py` imports recordings from a licensed Keeby installation.

## Commands

```text
keebyd                         Run the daemon and control panel
keebyd --headless              Run without opening a desktop window
keebyd --preview               Play the selected switch three times
keebyd --profile NAME          Override the configured profile
keebyd --list                  List installed profiles
keebyd --devices               List visible evdev devices
keebyd --render output.wav     Render the deterministic fidelity test
keebyd --config PATH           Use another configuration file
```

Closing the panel hides it in the notification tray. Select **Open Keebyd** from the tray menu or
launch Keebyd again from the application menu to bring it back. Use **Toggle mute** in the tray or
send `SIGUSR1` to the engine service. Send `SIGHUP` to reload the configuration and active profile.

You can also open the panel from a terminal:

```sh
systemctl --user restart keebyd-ui.service
```

## Architecture

The code keeps calculations separate from operating-system work:

- `src/audio.rs` owns bounded voice mixing, equal-power panning, tone filtering, and CPAL output.
- `src/sound.rs` parses sound packs and decodes audio into owned mono samples.
- `src/keymap.rs` is the pure evdev key-to-sound mapping.
- `src/config.rs` parses and writes the existing `key = value` format.
- `src/input.rs` adapts evdev events and hotplug rescans into typed monitor events.
- `src/web.rs` adapts the local Axum API and server-sent key events.
- `src/main.rs` owns resource lifetimes, signals, and application wiring.

The daemon starts its loopback control panel even when no audio device is available and retries
audio setup in the background. The panel API accepts only local hosts and same-origin browser
requests; it exposes raw key events and is not intended for network access. Settings changes and
SIGHUP reloads are serialized: a replacement profile is decoded before the new configuration and
profile are published together. Configuration files are replaced atomically. Input devices are
reconciled every two seconds so newly connected keyboards are discovered without a restart.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo build --locked
python3 tools/smoke.py
cargo build --release --locked
```

The smoke check runs the real renderer and headless control API with temporary sound packs. It
does not exercise actual keyboard capture or speaker output; verify those on a Linux machine with
devices. `tools/benchmark.py` compares an offline render against the reference DSP chain.
