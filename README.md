# keebyd — mechanical keyboard sounds for Linux

A faithful Linux recreation of [Keeby](https://getkeeby.com/) (mechanical
keyboard sounds on every keystroke), built by reverse engineering the Windows
app and porting its audio mechanism 1:1. **It now runs the real Keeby sound
packs** (all 22 recorded switch profiles) with the app's exact playback
semantics — verified by an automated benchmark.

```
you type  →  evdev (/dev/input)  →  keebyd  →  miniaudio → PipeWire/ALSA  →  🔊 thock
```

## What it does (feature parity with the original)

| Keeby (Windows/Mac)                  | keebyd (Linux)                                     |
|--------------------------------------|----------------------------------------------------|
| `WH_KEYBOARD_LL` global hook         | evdev monitor on `/dev/input/event*` (+ hotplug)   |
| key-repeat suppression               | identical (drops `value==2` auto-repeats)          |
| WASAPI 44.1 kHz float mixer          | miniaudio 44.1 kHz f32 mixer, ~6 ms period         |
| `<group>_<phase>_<NN>.wav` packs     | same pack format, bit-identical samples            |
| per-switch `NormalizationGain`       | ported from the decompiled catalog                 |
| spatial audio (pan by key position)  | same equal-power pan law (verified)                |
| per-key feel (home row softer)       | same `feelGain` curve + softness slider            |
| tone pad (LPF × pitch)               | same one-pole LPF + varispeed math                 |
| soft limiter on overlaps             | same tanh-above-0.9 limiter in the mix bus         |
| round-robin variation fallback       | same (falls back to alpha sounds)                  |
| Ctrl+K ×3 toggle shortcut            | same (800 ms window / 500 ms gap)                  |
| mouse click sounds                   | shared `_shared/mouse_*.wav`                       |
| enter overlay sounds                 | typewriter / faahh mp3s from `_shared/`            |
| notch overlay, 3D visualizer         | web UI with a live key-press visualizer (SSE)      |
| switch picker (brand/type/color)     | **web UI at http://127.0.0.1:7777**                |

## Install

```sh
bash tools/install.sh
```

That builds, installs to `~/.local/bin/keebyd`, and enables the systemd user
service. You need to be in the `input` group (`sudo usermod -aG input $USER`
+ re-login) to read `/dev/input/event*`.

**Open http://127.0.0.1:7777** — pick your switch, drag the tone pad, type.

## Sound packs

All 22 original Keeby profiles are installed to
`~/.local/share/keebyd/sounds/` by `tools/import-keeby.py` (from the app's
installer — use only if you have a license for the app). Custom packs are
drop-in directories of `alpha_down_01.wav`-style files plus a
`profile.conf` with `normalization_gain = x`.

Don't have the packs? `tools/synth_profiles.py` generates original
modal-synthesis switch packs (thocky-linear, clicky-blue, topre-soft, …).

## Controls

- **Ctrl+K ×3** — toggle mute (like the app's shortcut)
- **http://127.0.0.1:7777** — switch picker, tone pad, sliders, visualizer
- `kill -USR1 $(pidof keebyd)` — toggle mute; `kill -HUP` — reload config
- `keebyd --preview --profile NAME` — audition a profile from the terminal

## Benchmark

`tools/benchmark.py` proves fidelity against the decompiled semantics:

1. **Asset integrity** — all 606 imported files bit-identical to the app's
2. **Engine render vs reference** — the C engine's offline render
   (`keebyd --render`) compared stroke-by-stroke against a Python
   transcription of the decompiled `AudioEngine` DSP chain:
   global correlation **1.0000**, worst RMS delta **0.1%**, spectral delta
   **0.3%**, pan-law match **exact**
3. **Live latency** — keypress → sound onset measured over a PipeWire monitor
   capture: ~15 ms (the original runs an 80 ms WASAPI buffer)

## Layout

```
src/          keebyd daemon (C, miniaudio, evdev)
  audio.c     mixer: voices, varispeed, one-pole LPF, pan, limiter
  sound.c     decode (wav/mp3/flac) + profile loading
  input.c     evdev keyboard/mouse monitor + hotplug + hotkey
  keymap.c    linux keycodes → group/pan/feel (ported from keymap.json)
  catalog.c   the 22-switch metadata table (SwitchCatalog port)
  http.c      localhost UI server (+SSE visualizer stream)
  ui_embed.h  the picker UI, generated from tools/ui.html
tools/        synth_profiles.py, import-keeby.py, benchmark.py, uinput_type.py
re/           reverse engineering artifacts (extracted app + decompiled source)
```
