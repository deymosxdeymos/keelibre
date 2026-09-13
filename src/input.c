/*
 * input.c — evdev keyboard monitor.
 *
 * Keeby installs a global WH_KEYBOARD_LL hook and dedupes key repeats with a
 * pressed-set. On Linux the equivalent is reading /dev/input/event*: the
 * kernel gives us EV_KEY with value 1 (down) / 0 (up) / 2 (auto-repeat, which
 * we drop — identical behaviour to Keeby's repeat suppression).
 *
 * Also handled here:
 *  - keyboard device classification (has alpha keys, not a mouse/joystick)
 *  - hotplug via inotify on /dev/input (Keeby's DeviceChangeListener does the
 *    audio-device counterpart)
 *  - Ctrl+K triple-tap toggle (Keeby's default shortcut, 800/500 ms windows)
 */
#include "input.h"

#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <linux/input.h>
#include <poll.h>
#include <signal.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/inotify.h>
#include <sys/ioctl.h>
#include <time.h>
#include <unistd.h>

#define MAX_DEVICES 64
#define TAP_WINDOW_MS 800 /* Keeby ShortcutWindowMs */
#define TAP_GAP_MS    500 /* Keeby ShortcutGapMs     */

typedef struct {
    int  fd;
    char path[320];
    bool is_keyboard;
    bool is_mouse;
} Dev;

struct InputMonitor {
    Dev            devs[MAX_DEVICES];
    int            ndevs;
    int            inotify_fd;
    int            watch_fd;
    int            stop_pipe[2];
    HotkeyConfig   hotkey;
    InputCallback  on_key;
    HotkeyCallback on_hotkey;
    void          *userdata;

    /* hotkey state */
    bool ctrl_down;
    long taps[8];
    int  ntaps;
};

static long now_ms(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec * 1000L + ts.tv_nsec / 1000000L;
}

/* ---------- device classification ---------- */

/* A keyboard has a decent spread of alpha keys; mice only carry buttons. */
static bool classify_keyboard(int fd)
{
    uint8_t keybits[KEY_MAX / 8 + 1];
    memset(keybits, 0, sizeof(keybits));
    if (ioctl(fd, EVIOCGBIT(EV_KEY, sizeof(keybits)), keybits) < 0)
        return false;

    int alpha = 0;
    for (int k = KEY_Q; k <= KEY_P; k++)
        if (keybits[k / 8] & (1 << (k % 8))) alpha++;
    for (int k = KEY_A; k <= KEY_L; k++)
        if (keybits[k / 8] & (1 << (k % 8))) alpha++;
    bool has_space = keybits[KEY_SPACE / 8] & (1 << (KEY_SPACE % 8));
    bool has_mouse = keybits[BTN_LEFT / 8] & (1 << (BTN_LEFT % 8));
    return alpha >= 10 && has_space && !has_mouse;
}

static bool classify_mouse(int fd)
{
    uint8_t keybits[KEY_MAX / 8 + 1];
    memset(keybits, 0, sizeof(keybits));
    if (ioctl(fd, EVIOCGBIT(EV_KEY, sizeof(keybits)), keybits) < 0)
        return false;
    bool has_mouse = keybits[BTN_LEFT / 8] & (1 << (BTN_LEFT % 8));
    bool has_alpha = keybits[KEY_A / 8] & (1 << (KEY_A % 8));
    return has_mouse && !has_alpha;
}

static void scan_devices(InputMonitor *m)
{
    /* close all (inotify will re-trigger if a removed device reappears) */
    for (int i = 0; i < m->ndevs; i++)
        if (m->devs[i].fd >= 0) close(m->devs[i].fd);
    m->ndevs = 0;

    DIR *d = opendir("/dev/input");
    if (!d) {
        fprintf(stderr, "keebyd: cannot open /dev/input: %s\n", strerror(errno));
        return;
    }
    struct dirent *de;
    while ((de = readdir(d)) && m->ndevs < MAX_DEVICES) {
        if (strncmp(de->d_name, "event", 5) != 0) continue;
        char path[320];
        snprintf(path, sizeof(path), "/dev/input/%s", de->d_name);
        int fd = open(path, O_RDONLY | O_CLOEXEC | O_NONBLOCK);
        if (fd < 0) {
            if (errno != ENOENT)
                fprintf(stderr, "keebyd: %s: %s (are you in the 'input' group?)\n",
                        path, strerror(errno));
            continue;
        }
        bool kb = classify_keyboard(fd);
        bool mouse = false;
        if (!kb && getenv("KEEBY_MOUSE")) {
            mouse = classify_mouse(fd);
            if (mouse) kb = true; /* monitor mice too when requested */
        }
        if (getenv("KEEBY_DEBUG"))
            fprintf(stderr, "keebyd: %s -> %s\n", path,
                    kb ? (mouse ? "mouse" : "keyboard") : "other");
        if (!kb) {
            close(fd);
            continue;
        }
        m->devs[m->ndevs].fd = fd;
        snprintf(m->devs[m->ndevs].path, sizeof(m->devs[m->ndevs].path), "%s", path);
        m->devs[m->ndevs].is_keyboard = kb && !mouse;
        m->devs[m->ndevs].is_mouse = mouse;
        m->ndevs++;
    }
    closedir(d);
    fprintf(stderr, "keebyd: monitoring %d keyboard device%s\n",
            m->ndevs, m->ndevs == 1 ? "" : "s");
}

/* ---------- hotkey (port of Keeby KeyboardHook.CheckShortcut) ---------- */

static void check_hotkey(InputMonitor *m, uint16_t code, int phase)
{
    if (code == KEY_LEFTCTRL || code == KEY_RIGHTCTRL) {
        m->ctrl_down = phase == 0;
        return;
    }
    if (m->hotkey.hotkey_key < 0 || code != (uint16_t)m->hotkey.hotkey_key || phase != 0)
        return;
    if (m->hotkey.hotkey_ctrl && !m->ctrl_down) {
        m->ntaps = 0;
        return;
    }
    long t = now_ms();
    if (m->ntaps > 0 && t - m->taps[m->ntaps - 1] > TAP_GAP_MS)
        m->ntaps = 0;
    if (m->ntaps < (int)(sizeof(m->taps) / sizeof(m->taps[0])))
        m->taps[m->ntaps++] = t;
    int need = m->hotkey.hotkey_taps > 0 ? m->hotkey.hotkey_taps : 1;
    if (m->ntaps >= need) {
        if (t - m->taps[m->ntaps - need] <= TAP_WINDOW_MS) {
            m->ntaps = 0;
            if (m->on_hotkey)
                m->on_hotkey(m->userdata);
        } else if (m->ntaps > need - 1) {
            memmove(m->taps, m->taps + (m->ntaps - (need - 1)),
                    (size_t)(need - 1) * sizeof(long));
            m->ntaps = need - 1;
        }
    }
}

/* ---------- event loop ---------- */

static void handle_device_event(InputMonitor *m, Dev *dev)
{
    struct input_event ev;
    for (;;) {
        ssize_t n = read(dev->fd, &ev, sizeof(ev));
        if (n != (ssize_t)sizeof(ev)) return; /* EAGAIN / partial */
        if (ev.type != EV_KEY) continue;
        int phase;
        if (ev.value == 1) phase = 0;
        else if (ev.value == 0) phase = 1;
        else continue; /* value == 2: auto-repeat — suppressed like Keeby */

        check_hotkey(m, ev.code, phase);
        if (m->on_key) {
            InputEvent ie = { .code = ev.code, .phase = phase };
            m->on_key(ie, m->userdata);
        }
    }
}

static void handle_inotify(InputMonitor *m)
{
    char buf[4096] __attribute__((aligned(8)));
    for (;;) {
        ssize_t n = read(m->inotify_fd, buf, sizeof(buf));
        if (n <= 0) break;
    }
    /* debounce: devices appear slightly after the inotify event */
    struct timespec ts = { .tv_sec = 0, .tv_nsec = 250 * 1000 * 1000 };
    nanosleep(&ts, NULL);
    scan_devices(m);
}

InputMonitor *input_monitor_create(const HotkeyConfig *hotkey,
                                   InputCallback on_key,
                                   HotkeyCallback on_hotkey,
                                   void *userdata)
{
    InputMonitor *m = calloc(1, sizeof(*m));
    if (!m) return NULL;
    m->hotkey = hotkey ? *hotkey : (HotkeyConfig){ .hotkey_key = -1, .hotkey_taps = 1, .hotkey_ctrl = false };
    m->on_key = on_key;
    m->on_hotkey = on_hotkey;
    m->userdata = userdata;
    if (pipe(m->stop_pipe) != 0) {
        free(m);
        return NULL;
    }
    m->inotify_fd = inotify_init1(IN_NONBLOCK | IN_CLOEXEC);
    m->watch_fd = m->inotify_fd >= 0
        ? inotify_add_watch(m->inotify_fd, "/dev/input", IN_CREATE | IN_DELETE)
        : -1;
    scan_devices(m);
    return m;
}

void input_monitor_run(InputMonitor *m)
{
    struct pollfd fds[MAX_DEVICES + 2];
    for (;;) {
        int n = 0;
        for (int i = 0; i < m->ndevs; i++) {
            fds[n].fd = m->devs[i].fd;
            fds[n].events = POLLIN;
            fds[n].revents = 0;
            n++;
        }
        fds[n].fd = m->stop_pipe[0];
        fds[n].events = POLLIN;
        fds[n].revents = 0;
        int stop_slot = n;
        n++;
        if (m->inotify_fd >= 0) {
            fds[n].fd = m->inotify_fd;
            fds[n].events = POLLIN;
            fds[n].revents = 0;
            n++;
        }

        int rc = poll(fds, (nfds_t)n, -1);
        if (rc < 0) {
            if (errno == EINTR) continue;
            break;
        }
        if (fds[stop_slot].revents & POLLIN) break; /* stop requested */

        if (m->inotify_fd >= 0 && fds[n - 1].revents & POLLIN)
            handle_inotify(m);

        for (int i = 0; i < m->ndevs; i++) {
            if (fds[i].revents & (POLLIN | POLLHUP)) {
                if (fds[i].revents & POLLHUP) {
                    /* device gone: full rescan */
                    scan_devices(m);
                    break;
                }
                handle_device_event(m, &m->devs[i]);
            }
        }
    }
}

void input_monitor_stop(InputMonitor *m)
{
    if (m && m->stop_pipe[1] >= 0) {
        char c = 1;
        ssize_t rc = write(m->stop_pipe[1], &c, 1);
        (void)rc;
    }
}

void input_monitor_destroy(InputMonitor *m)
{
    if (!m) return;
    for (int i = 0; i < m->ndevs; i++)
        if (m->devs[i].fd >= 0) close(m->devs[i].fd);
    if (m->inotify_fd >= 0) close(m->inotify_fd);
    close(m->stop_pipe[0]);
    close(m->stop_pipe[1]);
    free(m);
}
