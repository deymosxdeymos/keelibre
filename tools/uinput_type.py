#!/usr/bin/env python3
"""
uinput_type.py — inject keystrokes through a virtual keyboard to test keebyd
end-to-end (evdev path) without touching the physical keyboard.
"""
import fcntl
import os
import struct
import sys
import time

UINPUT = "/dev/uinput"
UI_SET_EVBIT = 0x40045564
UI_SET_KEYBIT = 0x40045565
UI_DEV_CREATE = 0x5501
UI_DEV_DESTROY = 0x5502

EV_KEY = 0x01
EV_SYN = 0x00
SYN_REPORT = 0

KEY = {c: i for i, c in enumerate("abcdefghijklmnopqrstuvwxyz0123456789")}
# fix the map to linux keycodes: a=30..z=56 is wrong, real mapping:
LAYOUT = "qwertyuiop asdfghjkl zxcvbnm"
LAYOUT2 = "1234567890"
for i, c in enumerate(LAYOUT2):
    KEY[c] = 2 + i            # KEY_1=2 .. KEY_0=11
row1 = "qwertyuiop"
for i, c in enumerate(row1):
    KEY[c] = 16 + i           # KEY_Q=16 .. KEY_P=25
row2 = "asdfghjkl"
for i, c in enumerate(row2):
    KEY[c] = 30 + i           # KEY_A=30 .. KEY_L=38
row3 = "zxcvbnm"
for i, c in enumerate(row3):
    KEY[c] = 44 + i           # KEY_Z=44 .. KEY_M=50
KEY[" "] = 57
KEY["space"] = 57
KEY["enter"] = 28
KEY["."] = 52
KEY[";"] = 39
KEY["k_ctrl"] = 29  # left ctrl

fd = os.open(UINPUT, os.O_WRONLY | os.O_NONBLOCK)
fcntl.ioctl(fd, UI_SET_EVBIT, EV_KEY)
fcntl.ioctl(fd, UI_SET_EVBIT, EV_SYN)
# declare a full alphabet so keebyd classifies us as a keyboard
for code in list(range(1, 125)) + list(KEY.values()):
    fcntl.ioctl(fd, UI_SET_KEYBIT, code)

# uinput_user_dev: name[80] + input_id(8) + ff_effects_max(4) + abs arrays (64*4*4)
name = b"keebyd-test-kbd"
user_dev = name.ljust(80, b"\0") + struct.pack("HHHHI", 0x06, 0x1, 0x1, 0x1, 0) + b"\0" * (1024)
assert len(user_dev) == 1116
os.write(fd, user_dev)
fcntl.ioctl(fd, UI_DEV_CREATE)
time.sleep(0.1)


def emit(t, code, val):
    # input_event: timeval(16 bytes on 64-bit), u16 type, u16 code, s32 value
    os.write(fd, struct.pack("qqHHi", 0, 0, t, code, val))


def tap(sym):
    code = KEY[sym]
    emit(EV_KEY, code, 1)
    emit(EV_SYN, SYN_REPORT, 0)
    time.sleep(0.06)
    emit(EV_KEY, code, 0)
    emit(EV_SYN, SYN_REPORT, 0)
    time.sleep(0.14)


seq = sys.argv[1] if len(sys.argv) > 1 else "asdf"
for ch in seq:
    if ch == "_":
        time.sleep(0.3)
    else:
        tap(ch)

time.sleep(0.2)
fcntl.ioctl(fd, UI_DEV_DESTROY)
os.close(fd)
