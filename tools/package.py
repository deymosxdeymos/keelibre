#!/usr/bin/env python3
"""Create a native keebyd release archive using only the Python standard library."""

import argparse
import hashlib
import os
from pathlib import Path
import shutil
import stat
import tarfile
import tempfile
import zipfile


TARGETS = {
    "linux-x86_64": ("keebyd", "tar.gz"),
    "windows-x86_64": ("keebyd.exe", "zip"),
    "macos-aarch64": ("keebyd", "zip"),
    "macos-x86_64": ("keebyd", "zip"),
}
BUNDLE_ID = "io.github.deymosxdeymos.keebyd"


def write_plist(path: Path, version: str) -> None:
    path.write_text(f"""<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleExecutable</key><string>keebyd</string>
  <key>CFBundleIdentifier</key><string>{BUNDLE_ID}</string>
  <key>CFBundleName</key><string>Keebyd</string>
  <key>CFBundleDisplayName</key><string>Keebyd</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>{version}</string>
  <key>CFBundleVersion</key><string>{version}</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSInputMonitoringUsageDescription</key><string>Keebyd monitors keyboard input to play key sounds.</string>
</dict></plist>
""", encoding="utf-8")


def zip_tree(source: Path, archive: Path) -> None:
    with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as output:
        for path in sorted(source.rglob("*")):
            if not path.is_file():
                continue
            name = path.relative_to(source.parent).as_posix()
            info = zipfile.ZipInfo(name)
            info.date_time = (1980, 1, 1, 0, 0, 0)
            mode = path.stat().st_mode & 0o777
            info.external_attr = (stat.S_IFREG | mode) << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            output.writestr(info, path.read_bytes(), compresslevel=9)


def package(root: Path, target: str, binary: Path, output_dir: Path, version: str) -> Path:
    executable, extension = TARGETS[target]
    if not binary.is_file():
        raise SystemExit(f"binary does not exist: {binary}")
    packs = root / "packs"
    default_pack = packs / "thocky-linear"
    if not (default_pack / "profile.conf").is_file() or not any(default_pack.glob("*.wav")):
        raise SystemExit("default synthetic pack packs/thocky-linear is missing or invalid")

    name = f"keebyd-{version}-{target}"
    output_dir.mkdir(parents=True, exist_ok=True)
    archive = output_dir / f"{name}.{extension}"
    with tempfile.TemporaryDirectory() as temporary:
        staging = Path(temporary) / name
        staging.mkdir()
        shutil.copy2(root / "README.md", staging / "README.md")
        if target.startswith("macos-"):
            contents = staging / "Keebyd.app" / "Contents"
            (contents / "MacOS").mkdir(parents=True)
            shutil.copy2(binary, contents / "MacOS" / "keebyd")
            os.chmod(contents / "MacOS" / "keebyd", 0o755)
            shutil.copytree(packs, contents / "Resources" / "packs")
            write_plist(contents / "Info.plist", version)
        else:
            shutil.copy2(binary, staging / executable)
            if target == "linux-x86_64":
                os.chmod(staging / executable, 0o755)
            shutil.copytree(packs, staging / "packs")

        if extension == "zip":
            zip_tree(staging, archive)
        else:
            with tarfile.open(archive, "w:gz", format=tarfile.PAX_FORMAT) as output:
                output.add(staging, arcname=name)

    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    archive.with_name(archive.name + ".sha256").write_text(
        f"{digest}  {archive.name}\n", encoding="ascii"
    )
    return archive


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", required=True, choices=sorted(TARGETS))
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--version", required=True)
    parser.add_argument("--output-dir", type=Path, default=Path("dist"))
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    print(package(root, args.target, args.binary.resolve(), args.output_dir.resolve(), args.version))


if __name__ == "__main__":
    main()
