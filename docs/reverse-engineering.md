# Keeby reverse-engineering notes

Everything below was recovered from the Windows build (`KeebySetup.msi`,
v1.8.1, WiX/.NET 8 WPF) — `re/app/` holds the extracted install, `re/decompiled/`
the ILSpy output of `Keeby.dll`.

## What Keeby is

A paid macOS/Windows utility that plays recorded mechanical-switch sounds on
every keystroke system-wide. Mac version is App Store-only (`id6760791739`);
the Windows version ships as an MSI we can fully dissect.

## Architecture (from the decompiled sources)

```
KeyboardHook (user32!SetWindowsHookEx WH_KEYBOARD_LL)
  ├─ vkCode → Keymap.Lookup → KeyPosition {pan, group, feelGain}
  ├─ repeat suppression via a pressed-set (LL hook auto-fires on OS repeat)
  ├─ dispatch on the thread pool
  └─ 3×Ctrl+K tap detection (800 ms window, 500 ms gap, modifiers mask)
        ↓ KeyEvent
AudioEngine (NAudio)
  ├─ WasapiOut shared-mode, 44.1 kHz float stereo, 80 ms, MixingSampleProvider
  ├─ LoadProfile: <dir>/<group>_<phase>_<NN>.wav → CachedSound
  │    decode → StereoToMono(0.5/0.5) → WdlResample → 44100 → float[]
  │    (or .kbp: AES-GCM-decrypt first, then same path)
  ├─ Play(ev):
  │    round-robin variation per (group,phase); fallback (alpha,phase)
  │    chain: CachedSoundSampleProvider
  │           → PitchShiftSampleProvider (linear-interp varispeed, 0.88+toneY·0.24)
  │           → ToneLpfSampleProvider (one-pole, α=max(.06,c²), makeup=1+(1−c)²·3, dry=(1−c)·0.45)
  │           → PanningSampleProvider (SquareRootPan: cos/sin of (pan+1)·π/4)
  │           → VolumeSampleProvider (clamp(feel·norm·master, 0..4))
  ├─ feel = raw≥1 ? raw : 1+(raw−1)·HomeRowSoftness
  ├─ SoftLimiterSampleProvider: tanh(x) where |x|>0.9 (UI sounds + welcome)
  └─ extras: mouse hook sounds, wheel ticks (80 ms cooldown), enter overlay,
             notch UI chimes, welcome chime
Visualizer: AssimpNet loads .glb/.fbx 3D keyboards; keys pulse on events
Settings: %APPDATA%/Keeby/settings.json (JSON, mirrors the UI controls)
```

## Key position map (`Resources/keymap.json`)

78 VK codes → `{"pan": −0.95…+0.95, "group": …, "feelGain": …}`.
Comment in the file: *"Mirror of Mac KeyPositionMap.swift but with VK codes"*
— pan is the key's physical x position; feelGain encodes typing dynamics:
home row **0.40**, index stretch **1.30**, number row **1.55**, far stretch
**1.85**. Groups: alpha, space, enter, backspace, modifier, tab, arrow, mouse.
We ported this table 1:1 to Linux input keycodes (`src/keymap.c`) and filled
in ISO/JIS/numpad keys the Windows table couldn't name.

## Switch catalog (`SwitchCatalog.cs`)

22 profiles with display name, brand, type/force, UI color, optional
community contributor, and a per-profile `NormalizationGain` (0.55–3.5)
applied when volume normalization is on. Ported verbatim to `src/catalog.c`.

## Sound packs

Shipped as plain RIFF WAV (16-bit mono 44.1 kHz, 60–100 ms one-shots) under
`Resources/Sounds/<switch>/` — naming `<group>_<phase>_<NN>.wav`. Extra
groups (`caps_lock`, `command`, `control`, `escape`, `fn`, `function`) exist
in some packs but the loader ignores them (only the 8 groups parse).

Loudness reality check (gateron-ink-black alpha_down_01): peak 0.53,
RMS 0.033 — the samples are quiet, bright transients: ~1% energy < 500 Hz,
18% in 0.5–2 kHz, 82% > 2 kHz. (Our first two synthesized attempts failed
precisely here; measurements > intuition.)

## Paid-content protection (found, not bypassed)

- `.kbp` packs: `KBYP` magic, version 1, 12-byte AES-GCM nonce, ciphertext,
  16-byte tag, AAD = `<profile>/<file>.kbp` relative path.
- Content key: base64url claim from the license backend, unwrapped with
  HKDF-SHA256(wrapSecret, salt=nonce, info="keeby/ck/v1"). The wrap secret
  comes from the license server (Polar + custom backend), not the binary.
- Buyer watermark: FNV-1a(activationId|customer) seed → SplitMix64 → ±0.0005
  amplitude pattern. **Disabled in this build** (`Watermark.Enabled = false`,
  `Apply` is a no-op) — the MSI packs are plain WAVs.
- Licensing: license tokens from getkeeby.com/Polar, hardware fingerprint,
  offline activation window. Not relevant to the Linux port; we do not
  circumvent any of it (the WAVs ship unencrypted in the installer).

## Linux port decisions

- **evdev** replaces WH_KEYBOARD_LL: same data (keycode, down/up), auto-repeat
  arrives as `value==2` and is dropped — equivalent to Keeby's pressed-set
  dedupe. Hotplug via inotify; mice classified separately for click sounds.
- **miniaudio** replaces NAudio/WASAPI: same 44.1 kHz f32 mixer; period 256
  frames (~6 ms) instead of 80 ms — lower latency than the original.
- Voice model replaces MixingSampleProvider inputs; per-voice varispeed + LPF
  state replicate the provider chain sample-for-sample (proven by benchmark).
- The pan law bug (interleaved-channel indexing) was caught by the
  L/R-ratio regression in `tools/benchmark.py` and fixed.
- The switch-picker web UI mirrors the app's picker: brand grouping, hover
  preview, click-to-select burst, favorites, tone pad, normalization,
  per-key feel, mouse/enter sounds, live visualizer.

## Website findings (bonus)

The landing page demo plays `/thock-down.wav`/`/thock-up.wav` on keydown/up
with repeat suppression and skips INPUT/TEXTAREA targets unless the element
has `data-keeby-sounds`; a "thock counter" posts to `/api/thocks`. The
web-haptics demo synthesizes clicks: 4 ms white noise × exp decay through a
bandpass at (2000+2000·intensity) Hz, Q=8 — fine for haptics, not for keys.
