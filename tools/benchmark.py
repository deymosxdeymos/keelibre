#!/usr/bin/env python3
"""
benchmark.py — measure keebyd against the reference (decompiled Keeby semantics).

Three test layers:

  1. ASSET INTEGRITY  — every imported sample must be bit-identical to the
     original app's file (sha256).
  2. ENGINE RENDER    — keebyd --render writes a deterministic stroke pattern
      using the real engine code path (gain, varispeed, LPF, and pan).
     The reference implementation (below, transcribed from the decompiled
     AudioEngine/CachedSound/PitchShift/ToneLpf/SoftLimiter + NAudio pan law)
     renders the same pattern from the same files. Compare per-stroke:
       * peak ratio L/R (pan law regression)
       * per-stroke RMS delta
       * spectral band energy delta
       * sample-domain correlation
  3. LIVE LATENCY     — measure keypress -> sound-onset over the real
     monitor capture (uinput injection vs PipeWire monitor).

Usage: python3 benchmark.py [--sounds DIR] [--profile NAME] [--live]
"""
import argparse
import hashlib
import os
import struct
import subprocess
import sys
import wave

import numpy as np

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
KEEBYD = os.environ.get("KEEBYD_BIN", os.path.join(ROOT, "target", "release", "keebyd"))
SR = 44100

# decompiled SwitchCatalog normalization gains
NORM = {
    "aflion-carrot": 0.96, "akko-piano-pro": 3.0, "akko-cs-jelly-black": 3.5,
    "akko-v3-pro-cream-yellow": 3.5, "akko-clicky-pink": 0.9,
    "lofree-flow-2-surfer": 3.5, "lofree-flow-2-void": 3.5,
    "lofree-flow-2-pulse": 3.5, "alps-skcm-blue": 0.85,
    "drop-holy-panda": 0.9, "durock-alpaca": 1.0, "gateron-ink-black": 1.0,
    "gateron-ink-red": 1.0, "gateron-turquoise-tealios": 1.0,
    "ibm-buckling-spring": 0.7, "iqunix-mq80": 0.75, "kailh-box-navy": 0.8,
    "keychron-k2-max-red": 0.55, "keychron-k2-max-brown": 0.85,
    "lizard": 1.0, "novelkeys-cream": 1.0, "topre-classic": 1.0,
}

# the deterministic pattern in main.c render_benchmark (must stay in sync!)
PATTERN = [
    ("alpha", -0.72, 0.40), ("alpha", +0.60, 1.00), ("alpha", 0.00, 1.85),
    ("space", 0.00, 1.00), ("enter", +0.95, 1.00), ("backspace", +0.95, 1.00),
    ("tab", -0.90, 1.00), ("arrow", +0.70, 1.00), ("modifier", -0.80, 1.00),
]

# keeby defaults (Keeby ToneX/ToneY defaults)
TONE_LPF = 0.5     # ToneX default 0.5
TONE_PITCH = 1.0   # 0.88 + 0.5*0.24


def read_wav(path):
    w = wave.open(path)
    n, ch = w.getnframes(), w.getnchannels()
    x = np.frombuffer(w.readframes(n), dtype="<i2").astype(np.float64) / 32768.0
    w.close()
    return x.reshape(-1, ch)


def load_sample(path):
    """Reference CachedSound: mono mixdown 0.5/0.5, resample to 44.1k (cubic)."""
    x = read_wav(path)
    if x.shape[1] > 1:
        x = x.mean(axis=1)
    else:
        x = x[:, 0]
    return x


def resample_cubic(x, rate_from, rate_to=SR):
    if rate_from == rate_to:
        return x
    step = rate_from / rate_to
    n = int(len(x) / step)
    i = np.arange(n)
    src = i * step
    idx = src.astype(int)
    t = src - idx
    i0 = np.clip(idx - 1, 0, len(x) - 1)
    i1 = np.clip(idx, 0, len(x) - 1)
    i2 = np.clip(idx + 1, 0, len(x) - 1)
    i3 = np.clip(idx + 2, 0, len(x) - 1)
    a = 0.5 * (x[i2] - x[i0])
    b = x[i0] - 2.5 * x[i1] + 2 * x[i2] - 0.5 * x[i3]
    c = 0.5 * (x[i3] - x[i0]) + 1.5 * (x[i1] - x[i2])
    return ((c * t + b) * t + a) * t + x[i1]


def apply_tone(x, lpf, pitch):
    """Reference PitchShiftSampleProvider (linear varispeed) + ToneLpf."""
    if abs(pitch - 1.0) > 0.005:
        # linear interpolation varispeed, same as the decompiled provider
        n_out = int(len(x) / pitch)
        pos = np.arange(n_out) * pitch
        i0 = pos.astype(int)
        frac = pos - i0
        i1 = np.clip(i0 + 1, 0, len(x) - 1)
        i0 = np.clip(i0, 0, len(x) - 1)
        x = x[i0] * (1 - frac) + x[i1] * frac
    if lpf < 0.99:
        alpha = max(0.06, lpf * lpf)
        makeup = 1 + (1 - lpf) ** 2 * 3
        dry = (1 - lpf) * 0.45
        y = np.empty_like(x)
        state = 0.0
        for i in range(len(x)):
            state += alpha * (x[i] - state)
            y[i] = state * makeup + x[i] * dry
        x = y
    return x


def pan_gains(pan):
    """NAudio PanningSampleProvider default (SquareRootPanStrategy): cos/sin."""
    th = (pan + 1.0) * np.pi / 4
    return np.cos(th), np.sin(th)


def soft_limiter(x):
    x = x.copy()
    mask = np.abs(x) > 0.9
    x[mask] = np.tanh(x[mask])
    return x


def reference_render(profile_dir, master=1.0, norm=None):
    """Render PATTERN exactly like the decompiled AudioEngine.Play()."""
    if norm is None:
        norm = NORM.get(os.path.basename(profile_dir), 1.0)
    lead, gap, tail = int(0.1 * SR), int(0.3 * SR), int(0.5 * SR)
    total = lead + gap * 9 + tail
    out = np.zeros((total, 2))

    # preload groups (round-robin starts at variation 0 and increments per play)
    cache = {}
    rr = {}

    def sample_for(group, phase, slot):
        # main.c renders down strokes for all 9 groups, then up strokes
        pname = "down" if phase == 0 else "up"
        key = (group, phase)
        if key not in cache:
            files = []
            d = os.path.join(profile_dir, f"{group}_{pname}")
            if os.path.isdir(d):
                files = sorted(
                    os.path.join(d, f) for f in os.listdir(d)
                    if f.startswith(f"{group}_{pname}_") and f.endswith(".wav"))
            elif os.path.isfile(os.path.join(profile_dir, f"{group}_{pname}_01.wav")):
                files = [os.path.join(profile_dir, f"{group}_{pname}_01.wav")]
            # keeby pack layout: <dir>/<group>_<phase>_NN.wav
            if not files:
                files = sorted(
                    os.path.join(profile_dir, f) for f in os.listdir(profile_dir)
                    if f.startswith(f"{group}_{pname}_") and f.endswith(".wav"))
            cache[key] = [load_sample(f) for f in files]
        lst = cache[key]
        if not lst:
            lst = cache.get(("alpha", phase), [])
        if not lst:
            return None
        i = rr.get(key, 0)
        rr[key] = i + 1
        return lst[i % len(lst)]

    def submit(smp, pan, vol, at):
        if smp is None:
            return
        s = apply_tone(smp.copy(), TONE_LPF, TONE_PITCH)
        gl, gr = pan_gains(pan)
        end = min(at + len(s), total)
        out[at:end, 0] += s[: end - at] * gl * vol
        out[at:end, 1] += s[: end - at] * gr * vol

    for phase in (0, 1):
        for i, (group, pan, feel) in enumerate(PATTERN):
            smp = sample_for(group, phase, i)
            vol = min(max(feel * norm * master, 0.0), 4.0)
            at = lead + i * gap + (phase * int(0.235 * SR))
            submit(smp, pan, vol, at)

    # Normal switch playback goes directly into Keeby's mixer. The soft limiter
    # is only used by its UI notification sounds. The WAV writer clips the
    # rendered float stream to the signed 16-bit output range.
    return np.clip(out, -1.0, 1.0)


def band_energies(x, sr=SR):
    sp = np.abs(np.fft.rfft(x)) ** 2
    f = np.fft.rfftfreq(len(x), 1 / sr)
    return [sp[f < 500].sum(), sp[(f >= 500) & (f < 2000)].sum(), sp[f >= 2000].sum()]


def asset_integrity(orig_root, imp_root):
    """sha256 every shared profile file; imported must equal original."""
    total, ok = 0, 0
    for prof in sorted(os.listdir(orig_root)):
        d = os.path.join(orig_root, prof)
        if not os.path.isdir(d):
            continue
        di = os.path.join(imp_root, prof)
        if not os.path.isdir(di):
            print(f"  MISSING profile {prof}")
            continue
        for f in os.listdir(d):
            fo = os.path.join(d, f)
            fi = os.path.join(di, f)
            if not os.path.isfile(fi):
                continue
            total += 1
            ho = hashlib.sha256(open(fo, "rb").read()).hexdigest()
            hi = hashlib.sha256(open(fi, "rb").read()).hexdigest()
            if ho == hi:
                ok += 1
            else:
                print(f"  DIFF {prof}/{f}")
    return ok, total


def engine_render(profile, sounds, out_wav):
    cfg = f"/tmp/keeby-bench.conf"
    with open(cfg, "w") as f:
        f.write(f"profile = {profile}\nsounds_dir = {sounds}\nmaster_volume = 1.0\n")
    r = subprocess.run(
        [KEEBYD, "--config", cfg, "--profile", profile, "--render", out_wav],
        capture_output=True, text=True, timeout=60)
    if not os.path.exists(out_wav):
        print(r.stderr)
        raise RuntimeError("render failed")
    return read_wav(out_wav)


def compare(engine, reference, label):
    """Per-stroke metrics. Both are (N,2) float arrays."""
    n = min(len(engine), len(reference))
    e, r = engine[:n], reference[:n]
    print(f"\n== {label} ==")
    lead = int(0.08 * SR)
    worst = {"rms": 0, "band": 0, "pan": 0}
    for i, (gname, pan, _) in enumerate(PATTERN):
        for phase in (0, 1):
            at = lead + i * int(0.3 * SR) + phase * int(0.235 * SR)
            end = at + int(0.12 * SR)
            es, rs = e[at:end], r[at:end]
            if np.abs(rs).max() < 1e-6 and np.abs(es).max() < 1e-6:
                continue
            erms = np.sqrt((es ** 2).mean())
            rrms = np.sqrt((rs ** 2).mean())
            rms_delta = abs(erms - rrms) / max(rrms, 1e-9)
            eb = np.array(band_energies(es.mean(axis=1)))
            rb = np.array(band_energies(rs.mean(axis=1)))
            band_delta = np.abs(eb - rb).sum() / max(rb.sum(), 1e-9)
            # pan: L/R peak ratio
            epk = np.abs(es).max(axis=0)
            rpk = np.abs(rs).max(axis=0)
            eratio = epk[0] / max(epk[1], 1e-9)
            rratio = rpk[0] / max(rpk[1], 1e-9)
            pan_delta = abs(eratio - rratio)
            worst["rms"] = max(worst["rms"], rms_delta)
            worst["band"] = max(worst["band"], band_delta)
            worst["pan"] = max(worst["pan"], pan_delta)
            status = "OK " if (rms_delta < 0.15 and band_delta < 0.25 and pan_delta < 0.15) else "FAIL"
            print(f"  [{status}] {gname:9s} {'up ' if phase else 'dn '}  "
                  f"rmsΔ={rms_delta:5.1%} bandΔ={band_delta:5.1%} "
                  f"panL/R eng={eratio:5.2f} ref={rratio:5.2f}")
    # global correlation
    e = e - e.mean(); r = r - r.mean()
    denom = np.sqrt((e ** 2).sum() * (r ** 2).sum())
    corr = float((e * r).sum() / denom) if denom > 0 else 0.0
    print(f"  global corr={corr:.4f}  worst: rmsΔ={worst['rms']:.1%} "
          f"bandΔ={worst['band']:.1%} panΔ={worst['pan']:.3f}")
    return worst


def live_latency():
    """Measure keypress -> sound onset using uinput + monitor capture."""
    import time as t
    mon = subprocess.Popen(
        ["parecord", "--device=@DEFAULT_MONITOR@", "--file-format=wav",
         "--channels=2", "--rate=44100", "/tmp/keeby-lat.wav"],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    t.sleep(0.8)
    d = subprocess.Popen(
        [KEEBYD, "--config", os.path.expanduser("~/.config/keebyd/config.conf")],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    t.sleep(1.0)
    # inject one keypress
    subprocess.run(["python3", os.path.join(ROOT, "tools", "uinput_type.py", "a")])
    t.sleep(1.0)
    d.terminate(); mon.terminate()
    d.wait(); mon.wait()

    x = read_wav("/tmp/keeby-lat.wav").mean(axis=1)
    env = np.abs(x)
    k = int(0.002 * SR)
    sm = np.convolve(env, np.ones(k) / k, mode="same")
    onsets = np.where((sm[1:] > 0.01) & (sm[:-1] <= 0.01))[0]
    if len(onsets) == 0:
        print("  no onset found — check daemon/device")
        return
    print(f"  sound onset at {onsets[0] / SR * 1000:.0f} ms into capture "
          f"(capture starts before injection; delta vs previous run is the metric)")
    print(f"  device pipeline: period 256 frames = 5.8 ms + PipeWire/alsa overhead")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--sounds", default=os.path.expanduser("~/.local/share/keebyd/sounds"))
    ap.add_argument("--orig", default=os.path.join(ROOT, "re/app/LocalApp/Programs/Keeby/Resources/Sounds"))
    ap.add_argument("--profile", default="akko-piano-pro")
    ap.add_argument("--live", action="store_true")
    args = ap.parse_args()

    print("1) ASSET INTEGRITY")
    ok, total = asset_integrity(args.orig, args.sounds)
    print(f"  {ok}/{total} files bit-identical "
          f"{'— PASS' if ok == total else '— FAIL'}")

    print("\n2) ENGINE RENDER vs REFERENCE")
    out_wav = "/tmp/keeby-bench-render.wav"
    eng = engine_render(args.profile, args.sounds, out_wav)
    ref = reference_render(os.path.join(args.sounds, args.profile))
    # engine render length: lead + 9*gap/2 + tail — reference: lead + 9*gap + tail
    n = min(len(eng), len(ref)) // SR * SR
    compare(eng[:n], ref[:n], f"profile={args.profile}")

    if args.live:
        print("\n3) LIVE LATENCY")
        live_latency()


if __name__ == "__main__":
    main()
