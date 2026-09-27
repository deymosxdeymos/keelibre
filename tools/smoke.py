#!/usr/bin/env python3
"""Exercise the built daemon and offline renderer with disposable sound packs."""

import json
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import signal
import socket
import struct
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import wave


ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target/debug/keebyd"


def request(port, path, *, data=None, headers=None):
    body = json.dumps(data).encode() if data is not None else None
    req = urllib.request.Request(
        f"http://127.0.0.1:{port}{path}",
        data=body,
        headers={"Content-Type": "application/json", **(headers or {})},
        method="POST" if data is not None else "GET",
    )
    try:
        with urllib.request.urlopen(req, timeout=2) as response:
            return response.status, json.load(response) if path != "/" else None
    except urllib.error.HTTPError as error:
        return error.code, None


def pack(directory, amplitude):
    directory.mkdir()
    for phase in ("down", "up"):
        with wave.open(str(directory / f"alpha_{phase}_01.wav"), "wb") as output:
            output.setnchannels(1)
            output.setsampwidth(2)
            output.setframerate(44100)
            output.writeframes(amplitude.to_bytes(2, "little", signed=True) * 256)


def main():
    if not __debug__:
        raise SystemExit("Run without Python -O so behavior checks remain active")
    if not BIN.exists():
        raise SystemExit("Build first with cargo build --locked")
    with tempfile.TemporaryDirectory(prefix="keebyd-smoke-") as scratch:
        root = Path(scratch)
        sounds = root / "sounds"
        sounds.mkdir()
        pack(sounds / "first", 8000)
        pack(sounds / "second", 12000)
        pack(sounds / "preview", 10000)
        (sounds / "preview/alpha_down_02.wav").write_text("invalid audio")
        with socket.socket() as reservation:
            reservation.bind(("127.0.0.1", 0))
            port = reservation.getsockname()[1]
        config = root / "config.conf"
        config.write_text(f"profile = first\nsounds_dir = {sounds}\nui_port = {port}\n")

        rendered = root / "render.wav"
        subprocess.run(
            [str(BIN), "--config", str(config), "--render", str(rendered)],
            check=True,
            stdout=subprocess.DEVNULL,
        )
        with wave.open(str(rendered)) as output:
            assert output.getnchannels() == 2 and output.getframerate() == 44100
            output.setpos(4410)  # first stroke, with a deliberately left-biased pan
            frames = list(struct.iter_unpack("<hh", output.readframes(200)))
            left = sum(abs(frame[0]) for frame in frames)
            right = sum(abs(frame[1]) for frame in frames)
            assert left > right * 1.5 and right > 0, "render is silent or panning is wrong"

        with (root / "daemon.log").open("w+") as log:
            daemon = subprocess.Popen(
                [str(BIN), "--config", str(config), "--headless"],
                stdout=log,
                stderr=subprocess.STDOUT,
            )
            try:
                for _ in range(40):
                    if daemon.poll() is not None:
                        raise AssertionError("daemon exited during startup")
                    try:
                        if request(port, "/api/status")[0] == 200:
                            break
                    except urllib.error.URLError:
                        time.sleep(0.1)
                else:
                    raise AssertionError("control API did not start")

                assert request(port, "/api/status")[1]["profile"] == "first"
                status, settings = request(
                    port, "/api/settings", data={"enabled": False, "master_volume": 0.35}
                )
                assert status == 200 and not settings["enabled"]
                assert settings["master_volume"] == 0.35
                assert "enabled = false" in config.read_text()
                with ThreadPoolExecutor(max_workers=2) as pool:
                    changes = [
                        pool.submit(request, port, "/api/settings", data={"master_volume": 0.4}),
                        pool.submit(request, port, "/api/settings", data={"hover_preview": False}),
                    ]
                    assert all(change.result()[0] == 200 for change in changes)
                assert request(port, "/api/settings")[1]["master_volume"] == 0.4
                assert not request(port, "/api/settings")[1]["hover_preview"]
                assert request(port, "/api/select?name=second", data={})[0] == 200
                assert request(port, "/api/status")[1]["profile"] == "second"
                assert request(port, "/api/select?name=missing", data={})[0] != 200
                assert request(port, "/api/status")[1]["profile"] == "second"
                assert request(port, "/api/preview?name=preview", data={})[0] == 200
                assert request(port, "/api/select?name=preview", data={})[0] != 200
                assert request(port, "/api/status")[1]["profile"] == "second"
                assert request(port, "/api/status", headers={"Origin": "https://other.example"})[0] == 403
                assert request(port, "/api/status", headers={"Host": "other.example"})[0] == 403
                assert request(port, "/api/toggle-mute", data={})[1]["muted"]
                assert request(port, "/api/status")[1]["muted"]
                config.write_text(config.read_text().replace("profile = second", "profile = missing"))
                daemon.send_signal(signal.SIGHUP)
                for _ in range(40):
                    log.seek(0)
                    if "could not reload configuration" in log.read():
                        break
                    time.sleep(0.05)
                else:
                    raise AssertionError("SIGHUP reload was not attempted")
                assert request(port, "/api/status")[1]["profile"] == "second"
            except BaseException:
                log.flush()
                log.seek(0)
                print(log.read()[-4000:])
                raise
            finally:
                daemon.terminate()
                try:
                    daemon.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    daemon.kill()
                    daemon.wait()
    print("PASS: render, settings, selection, preview, access control, mute, failed reload")


if __name__ == "__main__":
    main()
