"""Archive contract tests; these do not pretend to compile native binaries."""

import hashlib
import importlib.util
from pathlib import Path
import plistlib
import stat
import tarfile
import tempfile
import unittest
import zipfile


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("release_package", ROOT / "tools/package.py")
PACKAGER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PACKAGER)


class PackageTests(unittest.TestCase):
    def test_archive_layouts_permissions_and_checksums(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            binary = root / "native-binary"
            binary.write_bytes(b"test executable placeholder")
            for target in PACKAGER.TARGETS:
                with self.subTest(target=target):
                    archive = PACKAGER.package(ROOT, target, binary, root, "0.1.0")
                    prefix = f"keebyd-0.1.0-{target}/"
                    if archive.name.endswith(".zip"):
                        with zipfile.ZipFile(archive) as contents:
                            files = {item.filename: contents.read(item) for item in contents.infolist()}
                            modes = {item.filename: item.external_attr >> 16 for item in contents.infolist()}
                    else:
                        with tarfile.open(archive) as contents:
                            files = {item.name: contents.extractfile(item).read() for item in contents if item.isfile()}
                            modes = {item.name: item.mode for item in contents if item.isfile()}
                    self.assertIn(prefix + "README.md", files)
                    if target.startswith("macos-"):
                        executable = prefix + "Keebyd.app/Contents/MacOS/keebyd"
                        packs = prefix + "Keebyd.app/Contents/Resources/packs/"
                        plist = plistlib.loads(files[prefix + "Keebyd.app/Contents/Info.plist"])
                        self.assertEqual(plist["CFBundleExecutable"], "keebyd")
                        self.assertEqual(plist["CFBundleShortVersionString"], "0.1.0")
                    else:
                        executable = prefix + ("keebyd.exe" if target.startswith("windows-") else "keebyd")
                        packs = prefix + "packs/"
                    self.assertEqual(files[executable], binary.read_bytes())
                    if not target.startswith("windows-"):
                        self.assertTrue(modes[executable] & stat.S_IXUSR)
                    for sample in (ROOT / "packs").rglob("*.wav"):
                        self.assertEqual(files[packs + sample.relative_to(ROOT / "packs").as_posix()], sample.read_bytes())
                    checksum = archive.with_name(archive.name + ".sha256").read_text().split()
                    self.assertEqual(checksum, [hashlib.sha256(archive.read_bytes()).hexdigest(), archive.name])

    def test_missing_binary_and_default_pack_fail_before_packaging(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            binary = root / "missing"
            with self.assertRaisesRegex(SystemExit, "binary does not exist"):
                PACKAGER.package(ROOT, "linux-x86_64", binary, root / "out", "0.1.0")
            binary.touch()
            with self.assertRaisesRegex(SystemExit, "default synthetic pack"):
                PACKAGER.package(root, "linux-x86_64", binary, root / "out", "0.1.0")
            self.assertFalse((root / "out").exists())


if __name__ == "__main__":
    unittest.main()
