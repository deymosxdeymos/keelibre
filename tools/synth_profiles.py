#!/usr/bin/env python3
"""
synth_profiles.py — synthesize original mechanical-switch sound packs for keebyd.

v2: modal synthesis. A keypress is an *impact* — the housing/case rings at a
handful of resonant frequencies, each an exponentially-decaying partial, plus a
~1ms low-passed noise transient for the attack texture. No raw noise bursts:
those are what made v1 sound like static and hurt ears.

Model per hit:
  body modes:   1-2 low partials (90-250 Hz), fast decay        -> the "thock"
  case modes:   600-1300 Hz short partial                       -> "knock"
  click mode:   1.8-5 kHz narrow partial, LOW gain              -> the switch
  attack:       0.3-1 ms of lowpassed noise, very quiet         -> texture
  space:        lower body mode + 2-4 delayed stabilizer taps

Writes keeby-compatible packs:  <out>/<profile>/<group>_<phase>_<NN>.wav
plus profile.conf with normalization_gain.

Usage: python3 synth_profiles.py [OUT_DIR]   (default: ./packs)
"""
import os
import sys
import wave

import numpy as np

SR = 44100
VARIANTS = 4
RNG = np.random.default_rng(20260913)

# ---------------------------------------------------------------------------
# modal synthesis core
# ---------------------------------------------------------------------------

def mode(freq, dur_s, q, gain, phase=None):
    """A single ringing mode: decaying sine, tau = Q / (pi*f)."""
    n = int(dur_s * SR)
    if n <= 0:
        return np.zeros(1)
    t = np.arange(n) / SR
    if phase is None:
        phase = RNG.uniform(0, 2 * np.pi)
    tau = q / (2 * np.pi * freq)  # e^-1 decay of amplitude
    env = np.exp(-t / tau)
    # prevent mode from ringing forever in the tail
    fade_n = min(n, int(0.003 * SR))
    env[:fade_n] *= np.linspace(0.35, 1.0, fade_n)  # 3ms soft attack to kill DC click
    return gain * env * np.sin(2 * np.pi * freq * t + phase)


def attack_transient(dur_s, cutoff_norm, gain):
    """Very short, low-passed noise tick — the physical 'contact' texture."""
    n = max(8, int(dur_s * SR))
    x = RNG.uniform(-1, 1, n)
    # one-pole LP applied twice for a gentler slope
    a = float(np.clip(cutoff_norm, 0.05, 0.9))
    s = 0.0
    for i in range(n):
        s += a * (x[i] - s)
        x[i] = s
    s = 0.0
    for i in range(n):
        s += a * (x[i] - s)
        x[i] = s
    env = np.exp(-np.arange(n) / (n / 3.5))
    return x * env * gain

# ---------------------------------------------------------------------------
# hit synthesis
# ---------------------------------------------------------------------------

def rms(x):
    return np.sqrt((x ** 2).mean()) + 1e-12


def rms_to(x, target):
    return x * (target / rms(x))


def lowpass_shave(x, cutoff_hz=9000):
    """Gentle linear-phase shave above cutoff via FFT."""
    sp = np.fft.rfft(x)
    freqs = np.fft.rfftfreq(len(x), 1 / SR)
    mask = np.clip((cutoff_hz * 1.6 - freqs) / (cutoff_hz * 0.6), 0, 1)
    return np.fft.irfft(sp * mask, len(x))


def hit(p, is_up=False, gain=1.0):
    dur = p.get("dur", 0.10) if not is_up else min(p.get("dur", 0.10), 0.07)
    out = np.zeros(int(dur * SR))

    def place(layer, rel):
        nonlocal out
        m = len(layer)
        out[:m] += rms_to(layer, rel)

    up_body = 0.55 if is_up else 1.0
    up_click = 0.5 if is_up else 1.0

    # --- body modes (the thock) --- RMS reference: 1.0
    body = np.zeros(int(dur * SR))
    for (f0, q, g) in p["body"]:
        f = f0 * RNG.uniform(0.97, 1.03) * (1.22 if is_up else 1.0)
        m = mode(f, dur, q, g * RNG.uniform(0.9, 1.1))
        body[: len(m)] += m
    place(body, 1.0 * up_body)

    # --- case knock mode ---
    f = p["knock_f"] * RNG.uniform(0.94, 1.06) * (1.15 if is_up else 1.0)
    m = mode(f, dur, p["knock_q"], 1.0 * RNG.uniform(0.85, 1.15))
    place(m, p["knock_rel"] * (0.6 if is_up else 1.0))

    # --- click mode (the switch element) ---
    if p["click_rel"] > 0:
        f = p["click_f"] * RNG.uniform(0.95, 1.05) * (1.3 if is_up else 1.0)
        m = mode(f, min(dur, 0.04), p["click_q"], 1.0 * RNG.uniform(0.8, 1.2))
        place(m, p["click_rel"] * up_click)

    # --- attack texture ---
    atk = attack_transient(0.0009 if not is_up else 0.0006, p["atk_cut"], 1.0)
    place(atk, p["atk_rel"] * (0.6 if is_up else 1.0))

    # shave fizz + smooth any truncation at the tail
    out = lowpass_shave(out)
    tail = min(len(out), int(0.006 * SR))
    out[-tail:] *= np.linspace(1.0, 0.0, tail)
    return out * gain


def stabilizer_rattle(sig, p):
    """Spacebar: 2-4 faint, slightly delayed re-strikes."""
    out = sig.copy()
    for _ in range(RNG.integers(2, 5)):
        d = int(RNG.uniform(0.005, 0.014) * SR)
        n = min(int(0.02 * SR), len(out) - d)
        if n <= 0:
            continue
        tap = mode(p["knock_f"] * RNG.uniform(0.9, 1.1), 0.02, p["knock_q"], 1.0)
        out[d : d + len(tap)] += rms_to(tap, 0.12)[:n]
    return out

# ---------------------------------------------------------------------------
# profiles — original "virtual switches"
# ---------------------------------------------------------------------------

def P(body, knock_f=900, knock_q=12, knock_rel=0.22,
      click_f=3000, click_q=18, click_rel=0.14,
      atk_cut=0.12, atk_rel=0.06, dur=0.10, norm=1.0):
    return dict(body=body, knock_f=knock_f, knock_q=knock_q, knock_rel=knock_rel,
                click_f=click_f, click_q=click_q, click_rel=click_rel,
                atk_cut=atk_cut, atk_rel=atk_rel, dur=dur, norm=norm)

PROFILES = {
    # deep muted linear — heavy aluminum case, foam-modded
    "thocky-linear": P(
        body=[(115, 9, 0.60), (185, 11, 0.18)],
        knock_f=760, knock_q=10, knock_rel=0.20,
        click_f=2100, click_q=14, click_rel=0.11,
        atk_cut=0.10, atk_rel=0.05, dur=0.10, norm=1.0),
    # round, friendly linear
    "creamy-linear": P(
        body=[(150, 10, 0.55), (255, 12, 0.20)],
        knock_f=920, knock_q=11, knock_rel=0.22,
        click_f=2700, click_q=16, click_rel=0.14,
        atk_cut=0.14, atk_rel=0.06, dur=0.09, norm=1.0),
    # sharp clicky — crisp but not piercing
    "clicky-blue": P(
        body=[(190, 10, 0.30), (330, 12, 0.12)],
        knock_f=1150, knock_q=13, knock_rel=0.26,
        click_f=3400, click_q=22, click_rel=0.34,
        atk_cut=0.20, atk_rel=0.07, dur=0.085, norm=0.85),
    # tactile bump
    "tactile-brown": P(
        body=[(140, 10, 0.48), (240, 12, 0.17)],
        knock_f=880, knock_q=11, knock_rel=0.24,
        click_f=2600, click_q=16, click_rel=0.18,
        atk_cut=0.15, atk_rel=0.06, dur=0.09, norm=0.95),
    # soft rubbery top-reh
    "topre-soft": P(
        body=[(95, 7, 0.65), (160, 9, 0.20)],
        knock_f=620, knock_q=9, knock_rel=0.15,
        click_f=1500, click_q=10, click_rel=0.06,
        atk_cut=0.07, atk_rel=0.04, dur=0.12, norm=1.05),
    # buckling spring: loud bright ping, Model-M soul
    "buckling-spring": P(
        body=[(220, 11, 0.25), (390, 13, 0.10)],
        knock_f=1350, knock_q=15, knock_rel=0.30,
        click_f=4600, click_q=26, click_rel=0.42,
        atk_cut=0.24, atk_rel=0.07, dur=0.08, norm=0.8),
    # hushed late-night tactile
    "silent-tactile": P(
        body=[(120, 8, 0.55), (200, 10, 0.16)],
        knock_f=700, knock_q=9, knock_rel=0.14,
        click_f=1700, click_q=12, click_rel=0.05,
        atk_cut=0.06, atk_rel=0.035, dur=0.10, norm=1.1),
    # low-profile thin tick
    "low-profile": P(
        body=[(280, 12, 0.22), (460, 13, 0.10)],
        knock_f=1500, knock_q=14, knock_rel=0.26,
        click_f=3200, click_q=18, click_rel=0.18,
        atk_cut=0.22, atk_rel=0.06, dur=0.06, norm=0.95),
}

GROUPS = ["alpha", "space", "enter", "backspace", "modifier", "tab", "arrow"]

# per-group multipliers (body_gain, click_gain, overall)
GROUP_TWEAK = {
    "alpha":     (1.00, 1.00, 1.00),
    "space":     (1.30, 0.80, 1.05),   # big bar, deep body, rattle added separately
    "enter":     (1.25, 1.00, 1.05),
    "backspace": (1.20, 1.00, 1.05),
    "modifier":  (0.90, 0.85, 0.90),
    "tab":       (0.95, 0.90, 0.90),
    "arrow":     (0.75, 0.85, 0.75),
}


def write_wav(path, x):
    x = np.clip(x, -1, 1)
    pcm = (x * 32767).astype("<i2")
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(pcm.tobytes())


def normalize_peak(x, peak):
    m = np.max(np.abs(x))
    return x * (peak / m) if m > 1e-9 else x


def synth_profile(out_root, name, p):
    d = os.path.join(out_root, name)
    os.makedirs(d, exist_ok=True)
    for g in GROUPS:
        body_mul, click_mul, out_mul = GROUP_TWEAK[g]
        for phase in ("down", "up"):
            for v in range(1, VARIANTS + 1):
                prof = dict(p)
                prof["body"] = [(f, q, gv * body_mul) for (f, q, gv) in p["body"]]
                prof["click_rel"] = p["click_rel"] * click_mul
                sig = hit(prof, is_up=(phase == "up"))
                if g == "space" and phase == "down":
                    sig = stabilizer_rattle(sig, p)
                # normalize each variant to a sane, consistent peak (-6 dB)
                sig = normalize_peak(sig, 0.5)
                write_wav(os.path.join(d, f"{g}_{phase}_{v:02d}.wav"), sig * out_mul)
    with open(os.path.join(d, "profile.conf"), "w") as f:
        f.write(f"# synthesized switch pack: {name} (modal synthesis, v2)\n")
        f.write(f"normalization_gain = {p['norm']:.2f}\n")
    print(f"wrote {d}")


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else "packs"
    os.makedirs(out, exist_ok=True)
    for name, p in PROFILES.items():
        synth_profile(out, name, p)
    print(f"\ndone → {out}/")


if __name__ == "__main__":
    main()
