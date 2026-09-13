/*
 * input.h — global keyboard capture via evdev (the Linux analogue of
 * Keeby's WH_KEYBOARD_LL SetWindowsHookEx hook).
 */
#ifndef KEEBY_INPUT_H
#define KEEBY_INPUT_H

#include <stdbool.h>
#include <stdint.h>

typedef struct {
    uint16_t code;  /* linux keycode */
    int      phase; /* 0 = down, 1 = up */
} InputEvent;

typedef struct InputMonitor InputMonitor;

typedef void (*InputCallback)(InputEvent ev, void *userdata);
typedef void (*HotkeyCallback)(void *userdata);

typedef struct {
    int  hotkey_key;      /* linux keycode, KEY_K default (Keeby VK 75)     */
    int  hotkey_taps;     /* taps required (Keeby default 3)                */
    bool hotkey_ctrl;     /* require Ctrl held (Keeby default modifiers=2)  */
} HotkeyConfig;

/*
 * Monitor all keyboard-like /dev/input/event* devices.
 *  on_key   — called for every key down/up (not repeats)
 *  on_hotkey— called when the toggle hotkey fires
 *  on_toggle— optional: called whenever mute toggles internally? (unused)
 */
InputMonitor *input_monitor_create(const HotkeyConfig *hotkey,
                                   InputCallback on_key,
                                   HotkeyCallback on_hotkey,
                                   void *userdata);

/* Blocks forever; returns on stop request (signal fd) or fatal error. */
void input_monitor_run(InputMonitor *m);
void input_monitor_stop(InputMonitor *m);
void input_monitor_destroy(InputMonitor *m);

#endif
