#!/usr/bin/env python3
"""Browser checks against a disposable real daemon. Requires agent-browser + Chrome.

Run cargo build --locked, then python3 tests/panel.py. No desktop input or
autostart settings are modified; all API writes go to a temporary config.
"""

import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
BIN = Path(os.environ.get("KEEBYD_BIN", ROOT / "target/debug/keebyd")).resolve()
SESSION = f"panel-{os.getpid()}"


def browser(*args):
    result = subprocess.run(
        ["agent-browser", "--session", SESSION, *args, "--json"],
        capture_output=True, text=True,
    )
    assert result.returncode == 0, (args, result.stdout, result.stderr)
    response = json.loads(result.stdout)
    assert response["success"], response
    return response["data"]


def evaluate(script):
    return browser("eval", script)["result"]


def check(script):
    assert evaluate(script), script


def navigate(tab):
    browser("click", f'[data-tab="{tab}"]')
    browser("wait", f'[data-panel="{tab}"]:not([hidden])')


def main():
    if not __debug__:
        raise SystemExit("Run without Python -O so assertions remain active")
    with tempfile.TemporaryDirectory(prefix="keebyd-panel-") as scratch:
        with socket.socket() as reservation:
            reservation.bind(("127.0.0.1", 0))
            port = reservation.getsockname()[1]
        url = f"http://127.0.0.1:{port}"
        config = Path(scratch) / "config.conf"
        config.write_text(
            f"profile = thocky-linear\nsounds_dir = {ROOT / 'packs'}\n"
            f"ui_port = {port}\nauto_start = false\nhover_preview = false\n"
        )

        def settings():
            with urllib.request.urlopen(url + "/api/settings", timeout=2) as response:
                return json.load(response)

        def saved():
            browser("wait", "--fn", "document.querySelector('#saveNote').textContent === 'Saved on your device.'")

        with (Path(scratch) / "daemon.log").open("w+") as log:
            daemon = subprocess.Popen(
                [str(BIN), "--headless", "--config", str(config)],
                stdout=log, stderr=subprocess.STDOUT,
            )
            try:
                for _ in range(100):
                    try:
                        settings()
                        break
                    except OSError:
                        assert daemon.poll() is None, "daemon exited"
                        time.sleep(.1)
                else:
                    raise AssertionError("daemon did not start")
                browser("open", url)
                browser("set", "viewport", "1180", "820", "2")
                browser("wait", "#panels:not([hidden])")
                check("document.querySelectorAll('.switch').length === 8")
                check("document.querySelector('#currentProfile').textContent === 'Thocky Linear'")
                check("document.querySelectorAll('.switch[aria-pressed=true]').length === 1")
                check("[...document.querySelectorAll('.switch')].every(x => x.getBoundingClientRect().bottom <= innerHeight)")

                browser("click", '[data-profile="clicky-blue"]')
                browser("wait", "--fn", "document.querySelector('#currentProfile').textContent === 'Clicky Blue'")
                assert settings()["profile"] == "clicky-blue"
                browser("click", "#preview")
                browser("click", "#power")
                saved()
                assert settings()["enabled"] is False
                check("document.querySelector('#power').getAttribute('aria-checked') === 'false'")
                print("PASS: real profile selection, preview endpoint and enabled persistence")

                # Asymmetric values expose swapped channels and lost debounced updates.
                for name, key in [("master_volume", "ArrowLeft"), ("mouse_volume", "Home")]:
                    browser("focus", f'input[name="{name}"]')
                    browser("press", key)
                saved()
                assert settings()["master_volume"] == .95
                assert settings()["mouse_volume"] == 0
                browser("focus", "#tone_lpf")
                browser("press", "ArrowRight")
                browser("focus", "#enter_tone_pitch")
                browser("press", "End")
                saved()
                assert settings()["tone_lpf"] == .51
                assert settings()["enter_tone_pitch"] == 1.12
                assert settings()["mouse_tone_pitch"] == 1.0
                browser("select", '[name="mouse_sound"]', "crisp")
                saved()
                assert settings()["mouse_sound"] == "crisp"
                print("PASS: keyboard-operated levels, independent tone axes and overlay selection")

                navigate("visualizer")
                browser("click", '[aria-label="Arctic theme"]')
                browser("click", '[aria-label="Top left"]')
                saved()
                assert settings()["visualizer_glow"] == "0A84FF"
                assert settings()["visualizer_position"] == "TopLeft"
                assert settings()["visualizer_follow_cursor"] is False
                check("document.activeElement.getAttribute('aria-label') === 'Top left'")
                # Exercise the same SSE consumer with a known physical key, then release it.
                evaluate("events.onmessage({data:JSON.stringify({code:30,phase:0})})")
                check("document.querySelector('#keyboard [data-code=\"30\"]').classList.contains('lit')")
                evaluate("events.onmessage({data:JSON.stringify({code:30,phase:1})})")
                check("!document.querySelector('#keyboard [data-code=\"30\"]').classList.contains('lit')")
                browser("click", '[aria-label="Show visualizer"]')
                saved()
                check("document.querySelector('#visualizer').hidden")
                assert settings()["visualizer_enabled"] is False
                print("PASS: palette, fixed position, retained focus, key-event rendering and off state")

                # A failed write must not disappear when an unrelated edit later succeeds.
                evaluate("window.originalFetch = window.fetch; window.fetch = (path, options) => path === '/api/settings' && options?.method === 'POST' ? Promise.resolve(new Response(JSON.stringify({error:'Injected write failure'}), {status:500})) : originalFetch(path, options)")
                browser("click", "#power")
                browser("wait", "--fn", "document.querySelector('#saveNote').textContent.includes('retry')")
                assert settings()["enabled"] is False
                evaluate("window.fetch = window.originalFetch")
                browser("click", '[aria-label="Bottom right"]')
                saved()
                assert settings()["enabled"] is True
                assert settings()["visualizer_position"] == "BottomRight"
                browser("reload")
                browser("wait", "#panels:not([hidden])")
                check("document.querySelector('#power').getAttribute('aria-checked') === 'true'")
                check("document.querySelector('[data-panel=visualizer]').hidden === false")
                print("PASS: failed-write feedback, retry preservation and reload/deep-link state")

                for tab in ["sound", "visualizer", "general", "extras", "whatsnew", "about"]:
                    navigate(tab)
                    check(f"document.querySelector('[data-panel={tab}]').hidden === false")
                    audit = browser("a11y", "--tags", "wcag2a,wcag2aa")
                    assert not audit["violations"], (tab, audit["violations"])
                print("PASS: six pages, zero axe WCAG A/AA violations")

                for width, height in [(760, 560), (390, 844)]:
                    browser("set", "viewport", str(width), str(height), "2")
                    for tab in ["sound", "visualizer", "general"]:
                        navigate(tab)
                        check("document.querySelector('#main').scrollWidth <= document.querySelector('#main').clientWidth")
                        check("document.documentElement.scrollWidth === innerWidth")
                print("PASS: native minimum and narrow layouts have no horizontal overflow")
                assert not browser("errors")["errors"]
                print("PASS: no uncaught browser errors")
            finally:
                subprocess.run(["agent-browser", "--session", SESSION, "close"], capture_output=True)
                daemon.terminate()
                daemon.wait(timeout=10)


if __name__ == "__main__":
    main()
