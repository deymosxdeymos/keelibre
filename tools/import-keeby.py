#!/usr/bin/env python3
"""
import-keeby.py — copy sound packs from a licensed Keeby installation into
keebyd's sounds directory.

The pack format is deliberately identical (the decompiled AudioEngine loads
`<group>_<phase>_<NN>.wav` from profile directories), so this is a straight
copy + a profile.conf carrying each profile's NormalizationGain from the
decompiled SwitchCatalog.

Only use this with sounds you have a license for (Keeby is a paid app; its
samples are the author's copyrighted work). If you own Keeby, point this at
its install directory:
  Windows:  %LOCALAPPDATA%/Programs/Keeby/Resources/Sounds
  macOS:    /Applications/Keeby.app/Contents/Resources/Sounds  (if shipped)

Usage: python3 import-keeby.py <keeby-sounds-dir> [keebyd-sounds-dir]
"""
import os
import shutil
import sys

# From decompiled Keeby.Audio.SwitchCatalog (v1.8.1)
NORM = {
    "aflion-carrot": 0.96, "akko-piano-pro": 3.0, "akko-cs-jelly-black": 3.5,
    "akko-v3-pro-cream-yellow": 3.5, "akko-clicky-pink": 0.9,
    "lofree-flow-2-surfer": 3.5, "lofree-flow-2-void": 3.5,
    "lofree-flow-2-pulse": 3.5, "alps-skcm-blue": 0.85,
    "drop-holy-panda": 0.9, "durock-alpaca": 1.0,
    "gateron-ink-black": 1.0, "gateron-ink-red": 1.0,
    "gateron-turquoise-tealios": 1.0, "ibm-buckling-spring": 0.7,
    "iqunix-mq80": 0.75, "kailh-box-navy": 0.8,
    "keychron-k2-max-red": 0.55, "keychron-k2-max-brown": 0.85,
    "lizard": 1.0, "novelkeys-cream": 1.0, "topre-classic": 1.0,
}


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(1)
    src = sys.argv[1]
    dst = sys.argv[2] if len(sys.argv) > 2 else os.path.expanduser(
        "~/.local/share/keebyd/sounds")
    if not os.path.isdir(src):
        print(f"not a directory: {src}")
        sys.exit(1)

    os.makedirs(dst, exist_ok=True)
    # loose sounds (mouse clicks, enter overlays, UI chimes) -> _shared/
    shared = os.path.join(dst, "_shared")
    os.makedirs(shared, exist_ok=True)
    for f in sorted(os.listdir(src)):
        p = os.path.join(src, f)
        if os.path.isfile(p) and f.lower().endswith((".wav", ".mp3")):
            shutil.copy2(p, os.path.join(shared, f))
    print(f"  _shared: loose ui/mouse/enter sounds")
    n = 0
    for name in sorted(os.listdir(src)):
        d = os.path.join(src, name)
        if not os.path.isdir(d):
            continue
        wav = [f for f in os.listdir(d) if f.lower().endswith(".wav")]
        if not wav:
            continue
        out = os.path.join(dst, name)
        os.makedirs(out, exist_ok=True)
        for f in wav:
            shutil.copy2(os.path.join(d, f), os.path.join(out, f))
        with open(os.path.join(out, "profile.conf"), "w") as fh:
            fh.write(f"# imported from licensed Keeby install ({name})\n")
            fh.write(f"normalization_gain = {NORM.get(name, 1.0):.2f}\n")
        print(f"  {name}: {len(wav)} wavs")
        n += 1
    print(f"\n{n} profiles -> {dst}")


if __name__ == "__main__":
    main()
