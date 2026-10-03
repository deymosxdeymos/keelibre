# keebyd

Cross-platform mechanical keyboard sounds, implemented in Rust. Keebyd reads native keyboard
events, mixes switch recordings at 44.1 kHz, and sends stereo audio through CPAL. A native webview
controls the active switch, tone, volume, spatial placement, and per-key response.

```text
keyboard -> native input -> keebyd -> CPAL -> system audio output
                              |
                              +-> native control panel
```

## Install

Release archives are produced for Windows x64, Linux x64, and macOS Apple Silicon and Intel when a
version tag is published. They contain the executable, README, and the included synthetic sound
packs (never recordings imported from a licensed installation). Builds are currently unsigned:
macOS Gatekeeper and Windows SmartScreen may warn or block them. Review the release checksum and
use your operating system's manual override only if you trust the download.

Once published, downloads appear on [GitHub Releases](https://github.com/deymosxdeymos/keelibre/releases).
Before a release, successful **Native CI** runs provide the same archives in their Artifacts section.
Extract the entire archive: run `keebyd.exe` on Windows, open `Keebyd.app` on macOS, or run
`./keebyd` on Linux. Keep the adjacent `packs` directory with the Windows/Linux executable.
The macOS app contains its packs internally. These are portable archives, not system installers.

On Windows, install the Microsoft Edge WebView2 Runtime if it is not already present. On Linux the
runtime libraries are `libasound2`, `libgtk-3-0`, and `libwebkit2gtk-4.1-0`. macOS uses its system
webview. Grant input access when prompted: **Accessibility** and **Input Monitoring** on macOS,
appropriate `/dev/input/event*` permissions on Linux, and normal desktop input access on Windows.

The Linux source installer requires Rust 1.90 or newer, ALSA development headers, GTK 3, and
WebKitGTK 4.1. On Debian or Ubuntu:

```sh
sudo apt install libasound2-dev libgtk-3-dev libwebkit2gtk-4.1-dev
# Install a current Rust toolchain with rustup first.
bash tools/install.sh
```

The installer builds a locked release, copies `keebyd` to `~/.local/bin`, and enables the systemd
user service and adds Keebyd to the application menu. Your user must be allowed to read
`/dev/input/event*`. Most distributions grant this
through the `input` group:

```sh
sudo usermod -aG input "$USER"
```

Membership permits reading all keyboard input, including passwords; grant it only to trusted users.
Log out and back in after changing groups. Launch **Keebyd** from the application menu once the
service starts.

Automatic startup is configured through systemd only by the Linux installer. On macOS and Windows,
add Keebyd manually in the operating system's Login Items or Startup Apps settings.

## Sound packs

Keebyd loads packs from its platform data directory by default. This is
`~/.local/share/keebyd/sounds` on Linux, `~/Library/Application Support/keebyd/sounds` on macOS, and
`%LOCALAPPDATA%\keebyd\sounds` on Windows. Configuration is at `~/.config/keebyd/config.conf` on
Linux, `~/Library/Application Support/keebyd/config.conf` on macOS, and
`%APPDATA%\keebyd\config.conf` on Windows. A pack contains files named
`<group>_<phase>_<variation>.wav`, for example `alpha_down_01.wav`. Supported groups are `alpha`,
`space`, `enter`, `backspace`, `modifier`, `tab`, `arrow`, and `mouse`. Linux respects
`XDG_CONFIG_HOME` and `XDG_DATA_HOME`. If the user sound directory does not exist, Keebyd uses the
packaged sounds beside the executable (or the macOS app's Resources directory). An explicit
`sounds_dir` setting always takes precedence.

An optional `profile.conf` sets pack loudness:

```ini
normalization_gain = 1.0
```

Eight ready-to-use synthetic packs are included in `packs/`. Install them with:

```sh
mkdir -p ~/.local/share/keebyd/sounds
cp -R packs/. ~/.local/share/keebyd/sounds/
```

`thocky-linear` is the default pack. `tools/install.sh` installs all included packs without
overwriting customized files.

To generate synthetic packs instead, install NumPy and run:

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
keebyd --devices               List Linux devices or describe the native global hook
keebyd --render output.wav     Render the deterministic fidelity test
keebyd --config PATH           Use another configuration file
```

Closing the panel hides it in the notification tray. Select **Open Keebyd** from the tray menu or
launch Keebyd again to bring it back. Use **Toggle mute** in the tray. On Linux, `SIGUSR1` toggles
mute and `SIGHUP` reloads the configuration and active profile; these signals are Linux-only.

**Quit panel** stops the engine when launched together; a separately managed headless engine keeps
running. macOS and Windows global hooks suppress repeated key-down events and do not capture
secure login screens. macOS permissions may need to be enabled manually and the app restarted.

With the Linux source installation, you can also open the panel from a terminal:

```sh
systemctl --user restart keebyd-ui.service
```

## Architecture

The code keeps calculations separate from operating-system work:

- `src/audio.rs` owns engine settings and profile publication.
- `src/audio/mixer.rs` owns deterministic mixing, panning, and tone filtering without OS I/O.
- `src/audio/output.rs` owns the CPAL worker and fixed-buffer conversion to device sample rates.
- `src/sound.rs` parses sound packs and decodes audio into owned mono samples.
- `src/keymap.rs` is the pure key-to-sound mapping.
- `src/config.rs` parses and writes the existing `key = value` format.
- `src/input.rs` defines the shared event contract; `src/input/linux.rs` handles evdev and
  `src/input/native.rs` adapts macOS/Windows hooks to the same physical key numbering.
- `src/web.rs` adapts the local Axum API and server-sent key events.
- `src/main.rs` owns resource lifetimes, signals, and application wiring.

The daemon starts its loopback control panel even when no audio device is available and retries
audio setup in the background. The panel API accepts only local hosts and same-origin browser
requests; it exposes raw key events and is not intended for network access. Settings changes and
SIGHUP reloads are serialized: a replacement profile is decoded before the new configuration and
profile are published together. Configuration files are replaced atomically. On Linux, input
devices are reconciled every two seconds so newly connected keyboards are
discovered without a restart.

Voice storage is reserved for 64 voices, and variation counters use a fixed group/phase table.
The device adapter uses fixed buffers, preserves the 44.1 kHz DSP chain, and converts to common
floating-point/integer output formats and device sample rates (including 48 kHz). This is not a
lock-free audio engine; hardware latency must still be measured on the target machine.

```text
src/              Rust application and library modules; unit tests beside their code
  audio/          Pure mixer and CPAL output worker
  input/          Platform-specific input adapters
assets/           Embedded control-panel HTML
packs/            Redistributable synthetic sound packs
packaging/linux/  Desktop entry and systemd user units
tests/            Process-level smoke checks and archive contract tests
tools/            Installation, packaging, sound-generation, and measurement scripts
docs/             Historical reverse-engineering notes
.github/workflows/ Native CI and tag-triggered release builds
```

## Development

```sh
cargo fmt --check
cargo test --locked --all-targets
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo build --locked
python3 tests/smoke.py # Linux: includes Unix signal checks
python3 -m unittest discover -s tests
cargo build --release --locked
```

Build on Windows with the MSVC Rust toolchain and Visual Studio C++ Build Tools; on macOS install
Xcode Command Line Tools. No Rust toolchain is needed to run a release archive. To package a native
build locally (choose the matching OS/architecture and `.exe` on Windows):

```sh
python3 tools/package.py --target linux-x86_64 --binary target/release/keebyd --version 0.1.0
```

The script writes the archive and SHA-256 checksum to `dist/`. The release workflow runs on a
`vVERSION` tag matching `Cargo.toml`, tests/builds all four native targets, and publishes only after
all succeed. Pushing a version tag publishes a GitHub release; local packaging does not.

The smoke check runs the real renderer and headless control API with temporary sound packs.
Native CI is configured to compile and test on each supported operating system, but does not prove keyboard
capture, permission prompts, speaker output, tray behavior, or startup integration on real devices;
verify those manually on each platform. `tools/benchmark.py` compares an offline render against the
reference DSP chain.
