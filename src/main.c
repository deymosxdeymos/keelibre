/*
 * keebyd — mechanical keyboard sounds for Linux.
 *
 * A faithful port of Keeby's audio mechanism to Linux:
 *   WH_KEYBOARD_LL hook  -> evdev /dev/input/event* monitor
 *   NAudio WASAPI mixer  -> miniaudio f32 mixer with the same per-hit DSP
 *   keymap.json (VK)     -> built-in Linux keycode keymap
 *
 * Copyright (c) 2026 — original implementation, for study/personal use.
 */
#include <errno.h>
#include <stdarg.h>
#include <linux/input-event-codes.h>
#include <signal.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>
#include <dirent.h>
#include "ui_embed.h"
#include <unistd.h>

#include "audio.h"
#include "catalog.h"
#include "config.h"
#include "http.h"
#include "input.h"
#include "keymap.h"
#include "sound.h"

/* timestamped logging */
static void klog(const char *fmt, ...)
{
    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);
    struct tm tm;
    localtime_r(&ts.tv_sec, &tm);
    fprintf(stderr, "[%02d:%02d:%02d.%03ld] ", tm.tm_hour, tm.tm_min, tm.tm_sec, ts.tv_nsec / 1000000);
    va_list ap;
    va_start(ap, fmt);
    vfprintf(stderr, fmt, ap);
    va_end(ap);
    fputc('\n', stderr);
}

static InputMonitor *g_monitor;
static AudioEngine  *g_engine;
static Config        g_cfg;
static atomic_int    g_reload;
static atomic_int    g_toggle_mute;
static char          g_config_path[512];
static int           g_sse_fd = -1; /* visualizer client */

static void on_signal(int sig)
{
    (void)sig;
    if (g_monitor) input_monitor_stop(g_monitor);
}

static void on_usr1(int sig)
{
    (void)sig;
    atomic_store(&g_toggle_mute, 1);
    /* wake the poll loop */
    if (g_monitor) input_monitor_stop(g_monitor);
}

static void on_hup(int sig)
{
    (void)sig;
    atomic_store(&g_reload, 1);
    if (g_monitor) input_monitor_stop(g_monitor);
}

static void install_profile(AudioEngine *engine, const Config *cfg)
{
    char dir[768];
    snprintf(dir, sizeof(dir), "%s/%s", cfg->sounds_dir, cfg->profile);
    Profile *prof = malloc(sizeof(Profile));
    if (!prof) return;
    if (profile_load(prof, dir) != 0) {
        free(prof);
        klog("profile '%s' not found in %s\n", cfg->profile, cfg->sounds_dir);
        klog("run tools/synth_profiles.py <dir> to generate packs, or set sounds_dir=\n");
        return;
    }
    audio_engine_set_profile(engine, prof);
}

/* ---------- offline render (--render): deterministic benchmark path ---------- */

static void write_wav_stereo(const char *path, const float *data, size_t frames)
{
    FILE *f = fopen(path, "wb");
    if (!f) return;
    uint32_t sr = KEEBY_SAMPLE_RATE;
    uint32_t data_sz = (uint32_t)(frames * 4);
    uint8_t hdr[44] = {
        'R','I','F','F', 0,0,0,0, 'W','A','V','E', 'f','m','t',' ',
        16,0,0,0, 1,0, 2,0, 0,0,0,0, 0,0,0,0, 4,0, 16,0,
        'd','a','t','a', 0,0,0,0,
    };
    memcpy(hdr + 4, &(uint32_t){36 + data_sz}, 4);
    memcpy(hdr + 24, &sr, 4);
    memcpy(hdr + 28, &sr, 4); /* byte rate = sr*4 */
    memcpy(hdr + 40, &data_sz, 4);
    fwrite(hdr, 1, 44, f);
    for (size_t i = 0; i < frames * 2; i++) {
        float cl = data[i] < -1 ? -1 : (data[i] > 1 ? 1 : data[i]);
        int16_t s16 = (int16_t)(cl * 32767);
        fwrite(&s16, 2, 1, f);
    }
    fclose(f);
}

/* Deterministic script: every group×phase with known pan/feel, 300 ms apart. */
static int render_benchmark(const char *outpath, float master, float norm)
{
    static const struct { KeyGroup g; const char *name; float pan; float feel; } seq[] = {
        { GRP_ALPHA,     "alpha",     -0.72f, 0.40f },
        { GRP_ALPHA,     "alpha",     +0.60f, 1.00f },
        { GRP_ALPHA,     "alpha",      0.00f, 1.85f },
        { GRP_SPACE,     "space",      0.00f, 1.00f },
        { GRP_ENTER,     "enter",     +0.95f, 1.00f },
        { GRP_BACKSPACE, "backspace", +0.95f, 1.00f },
        { GRP_TAB,       "tab",       -0.90f, 1.00f },
        { GRP_ARROW,     "arrow",     +0.70f, 1.00f },
        { GRP_MODIFIER,  "modifier",  -0.80f, 1.00f },
    };
    const int stroke_gap = (int)(0.30f * KEEBY_SAMPLE_RATE);
    const int tail = (int)(0.5f * KEEBY_SAMPLE_RATE);
    const int lead = (int)(0.1f * KEEBY_SAMPLE_RATE);
    size_t total = lead + stroke_gap * 9 + tail;
    float *buf = calloc(total * 2, sizeof(float));

    for (int ph = 0; ph < 2; ph++) {
        for (size_t i = 0; i < sizeof(seq) / sizeof(seq[0]); i++) {
            int at = lead + ((int)i * 2 + ph) * stroke_gap / 2;
            /* phase down at even slots, up 85 ms later — same stroke model */
            at += ph * (int)(0.085f * KEEBY_SAMPLE_RATE);
            audio_engine_play(g_engine, seq[i].g, ph, seq[i].pan, seq[i].feel);
            /* mix until voice finishes: pump the mixer in 128-frame chunks */
            for (int t = 0; t < stroke_gap / 2; t += 128) {
                if ((size_t)(at + 128) * 2 > total * 2) break;
                keeby_engine_mix(g_engine, buf + (size_t)at * 2, 128);
                at += 128;
            }
        }
    }
    write_wav_stereo(outpath, buf, total);
    free(buf);
    (void)master; (void)norm;
    return 0;
}


/* ---------- switch-picker UI API ---------- */

static const char *g_ui_html = UI_HTML; /* embedded at build time (ui_embed.h) */

static void json_escape(const char *in, char *out, size_t cap)
{
    size_t o = 0;
    for (const char *p = in; *p && o + 8 < cap; p++) {
        if (*p == '"' || *p == '\\') { out[o++] = '\\'; }
        out[o++] = *p;
    }
    out[o] = 0;
}

static int dir_has_wavs(const char *path)
{
    DIR *d = opendir(path);
    if (!d) return 0;
    struct dirent *de;
    int found = 0;
    while ((de = readdir(d))) {
        size_t n = strlen(de->d_name);
        if (n > 4 && strcasecmp(de->d_name + n - 4, ".wav") == 0) { found = 1; break; }
    }
    closedir(d);
    return found;
}

static int dir_has_wavs_at(const char *root, const char *name)
{
    if (!strcmp(name, "_shared")) return 0;
    char p[768];
    snprintf(p, sizeof(p), "%s/%s", root, name);
    return dir_has_wavs(p);
}

static int is_favorite(const char *name)
{
    const char *p = g_cfg.favorites;
    size_t n = strlen(name);
    while (*p) {
        const char *comma = strchr(p, ',');
        size_t seg = comma ? (size_t)(comma - p) : strlen(p);
        if (seg == n && strncmp(p, name, n) == 0) return 1;
        p = comma ? comma + 1 : p + seg;
    }
    return 0;
}

static void toggle_favorite(const char *name, int on)
{
    char buf[512] = { 0 };
    const char *p = g_cfg.favorites;
    char *w = buf;
    while (*p) {
        const char *comma = strchr(p, ',');
        size_t seg = comma ? (size_t)(comma - p) : strlen(p);
        int match = seg == strlen(name) && strncmp(p, name, seg) == 0;
        if (!match && w + seg + 2 < buf + sizeof(buf)) {
            if (w != buf) *w++ = ',';
            memcpy(w, p, seg);
            w += seg;
        }
        p = comma ? comma + 1 : p + seg;
    }
    if (on && w + strlen(name) + 2 < buf + sizeof(buf)) {
        if (w != buf) *w++ = ',';
        strcpy(w, name);
    }
    snprintf(g_cfg.favorites, sizeof(g_cfg.favorites), "%s", buf);
}

static void api_profiles(int fd)
{
    char out[16384];
    size_t o = (size_t)snprintf(out, sizeof(out), "{\"profiles\":[");
    char dir[768];
    DIR *d = opendir(g_cfg.sounds_dir);
    if (d) {
        struct dirent *de;
        int first = 1;
        while ((de = readdir(d))) {
            if (de->d_name[0] == '.' || !dir_has_wavs_at(g_cfg.sounds_dir, de->d_name)) continue;
            const SwitchMeta *m = catalog_find(de->d_name);
            char disp[128], brand[64], type[96], col[16], contrib[128];
            if (m) {
                json_escape(m->display, disp, sizeof(disp));
                json_escape(m->brand, brand, sizeof(brand));
                json_escape(m->type, type, sizeof(type));
                json_escape(m->color, col, sizeof(col));
                json_escape(m->contributor, contrib, sizeof(contrib));
            } else {
                json_escape(de->d_name, disp, sizeof(disp));
                snprintf(brand, sizeof(brand), "Other");
                snprintf(type, sizeof(type), "Custom pack");
                snprintf(col, sizeof(col), "#777777");
                snprintf(contrib, sizeof(contrib), "");
            }
            int remain = (int)(sizeof(out) - o - 512);
            if (remain <= 0) break;
            o += (size_t)snprintf(out + o, remain, "%s{\"name\":\"%s\",\"display\":\"%s\","
                                  "\"brand\":\"%s\",\"type\":\"%s\",\"color\":\"%s\","
                                  "\"contributor\":\"%s\",\"favorite\":%s,\"norm\":%.2f}",
                                  first ? "" : ",", de->d_name, disp, brand, type, col,
                                  contrib, is_favorite(de->d_name) ? "true" : "false",
                                  m ? m->norm : 1.0f);
            first = 0;
        }
        closedir(d);
    }
    snprintf(out + o, sizeof(out) - o, "]}");
    http_respond(fd, "200 OK", "application/json", out, strlen(out));
}

static void api_settings(int fd)
{
    char out[1024];
    snprintf(out, sizeof(out),
             "{\"profile\":\"%s\",\"master_volume\":%.2f,\"enabled\":%s,"
             "\"spatial_audio\":%s,\"per_key_feel\":%s,\"home_row_softness\":%.2f,"
             "\"volume_normalization\":%s,\"mute_modifiers\":%s,\"tone_lpf\":%.3f,"
             "\"tone_pitch\":%.3f,\"mouse_clicks\":%s,\"favorites\":\"%s\","
             "\"hover_preview\":%s,\"enter_sound\":\"%s\",\"enter_volume\":%.2f}",
             g_cfg.profile, g_cfg.master_volume, g_cfg.enabled ? "true" : "false",
             g_cfg.spatial_audio ? "true" : "false", g_cfg.per_key_feel ? "true" : "false",
             g_cfg.home_row_softness, g_cfg.volume_normalization ? "true" : "false",
             g_cfg.mute_modifiers ? "true" : "false", g_cfg.tone_lpf, g_cfg.tone_pitch,
             g_cfg.mouse_clicks ? "true" : "false", g_cfg.favorites,
             g_cfg.hover_preview ? "true" : "false", g_cfg.enter_sound, g_cfg.enter_volume);
    http_respond(fd, "200 OK", "application/json", out, strlen(out));
}

/* extract "key":value (number, bool or string) from a small json body */
static int json_get(const char *body, const char *key, char *val, size_t cap)
{
    char pat[64];
    snprintf(pat, sizeof(pat), "\"%s\"", key);
    const char *p = strstr(body, pat);
    if (!p) return 0;
    p += strlen(pat);
    while (*p == ' ' || *p == ':') p++;
    if (*p == '"') {
        p++;
        const char *e = strchr(p, '"');
        if (!e) return 0;
        size_t n = (size_t)(e - p) < cap - 1 ? (size_t)(e - p) : cap - 1;
        memcpy(val, p, n);
        val[n] = 0;
        return 1;
    }
    const char *e = p;
    while (*e && *e != ',' && *e != '}' && *e != ' ') e++;
    size_t n = (size_t)(e - p) < cap - 1 ? (size_t)(e - p) : cap - 1;
    memcpy(val, p, n);
    val[n] = 0;
    return 1;
}

static void apply_setting(const char *key, const char *val)
{
    float f = strtof(val, NULL);
    if (!strcmp(key, "master_volume")) g_cfg.master_volume = f;
    else if (!strcmp(key, "enabled")) g_cfg.enabled = atoi(val) || !strcmp(val, "true");
    else if (!strcmp(key, "spatial_audio")) g_cfg.spatial_audio = atoi(val) || !strcmp(val, "true");
    else if (!strcmp(key, "per_key_feel")) g_cfg.per_key_feel = atoi(val) || !strcmp(val, "true");
    else if (!strcmp(key, "home_row_softness")) g_cfg.home_row_softness = f;
    else if (!strcmp(key, "volume_normalization")) g_cfg.volume_normalization = atoi(val) || !strcmp(val, "true");
    else if (!strcmp(key, "mute_modifiers")) g_cfg.mute_modifiers = atoi(val) || !strcmp(val, "true");
    else if (!strcmp(key, "tone_lpf")) g_cfg.tone_lpf = f;
    else if (!strcmp(key, "tone_pitch")) g_cfg.tone_pitch = f;
    else if (!strcmp(key, "mouse_clicks")) g_cfg.mouse_clicks = atoi(val) || !strcmp(val, "true");
    else if (!strcmp(key, "hover_preview")) g_cfg.hover_preview = atoi(val) || !strcmp(val, "true");
    else if (!strcmp(key, "enter_sound")) snprintf(g_cfg.enter_sound, sizeof(g_cfg.enter_sound), "%s", val);
    else if (!strcmp(key, "enter_volume")) g_cfg.enter_volume = f;
    else return;

    /* push into engine live */
    AudioSettings as = {
        .spatial_audio = g_cfg.spatial_audio, .per_key_feel = g_cfg.per_key_feel,
        .volume_normalization = g_cfg.volume_normalization, .mute_modifiers = g_cfg.mute_modifiers,
        .home_row_softness = g_cfg.home_row_softness, .master_volume = g_cfg.master_volume,
        .tone_lpf = g_cfg.tone_lpf, .tone_pitch = g_cfg.tone_pitch,
        .enabled = g_cfg.enabled, .muted = false,
    };
    audio_engine_apply_settings(g_engine, &as);
    config_save(&g_cfg, g_config_path);
}

static void route(const HttpRequest *req, int fd)
{
    if (g_sse_fd >= 0 && g_sse_fd != fd) { close(g_sse_fd); g_sse_fd = -1; }

    if (strcmp(req->path, "/api/events") == 0) {
        http_respond(fd, "200 OK", "text/event-stream", "", 0);
        (void)!write(fd, "retry: 1000\n\n", 13);
        g_sse_fd = fd;
        return; /* keep open */
    }
    if (strcmp(req->path, "/") == 0) {
        if (g_ui_html) http_respond(fd, "200 OK", "text/html; charset=utf-8", g_ui_html, strlen(g_ui_html));
        else http_respond(fd, "404 Not Found", "text/plain", "ui missing", 10);
        return;
    }
    if (strcmp(req->path, "/api/status") == 0) {
        char out[256];
        snprintf(out, sizeof(out), "{\"profile\":\"%s\",\"muted\":%s,\"enabled\":%s}",
                 g_cfg.profile, audio_engine_get_muted(g_engine) ? "true" : "false",
                 g_cfg.enabled ? "true" : "false");
        respond_json(fd, out);
        return;
    }
    if (strcmp(req->path, "/api/profiles") == 0) { api_profiles(fd); return; }
    if (strcmp(req->path, "/api/settings") == 0) {
        if (strcmp(req->method, "POST") == 0 && req->body) {
            static const char *keys[] = {
                "master_volume", "enabled", "spatial_audio", "per_key_feel",
                "home_row_softness", "volume_normalization", "mute_modifiers",
                "tone_lpf", "tone_pitch", "mouse_clicks", "hover_preview",
                "enter_sound", "enter_volume", NULL
            };
            for (int i = 0; keys[i]; i++) {
                char val[256];
                if (json_get(req->body, keys[i], val, sizeof(val)))
                    apply_setting(keys[i], val);
            }
        }
        api_settings(fd);
        return;
    }

    if (strcmp(req->path, "/api/select") == 0) {
        char name[128] = { 0 };
        strncpy(name, req->query, sizeof(name) - 1);
        char *v = strstr(name, "name=");
        if (v) {
            urldecode(v + 5);
            snprintf(g_cfg.profile, sizeof(g_cfg.profile), "%s", v + 5);
            install_profile(g_engine, &g_cfg);
            config_save(&g_cfg, g_config_path);
            respond_json(fd, "{\"ok\":true}");
            return;
        }
        http_respond(fd, "400 Bad Request", "text/plain", "missing name", 12);
        return;
    }
    if (strcmp(req->path, "/api/preview") == 0) {
        char q1[512], q2[512];
        snprintf(q1, sizeof(q1), "%s", req->query);
        snprintf(q2, sizeof(q2), "%s", req->query);
        char *name = query_get(q1, "name");
        char *mode = query_get(q2, "mode");
        if (name) {
            char dir[768];
            snprintf(dir, sizeof(dir), "%s/%s", g_cfg.sounds_dir, name);
            if (mode && strcmp(mode, "hover") == 0)
                audio_engine_hover_preview(g_engine, dir);
            else {
                /* preview the other profile: load temporarily like Keeby PlayPreview */
                audio_engine_hover_preview(g_engine, dir);
                struct timespec ts = { .tv_sec = 0, .tv_nsec = 180 * 1000 * 1000 };
                nanosleep(&ts, NULL);
                audio_engine_hover_preview(g_engine, dir);
                nanosleep(&ts, NULL);
                audio_engine_hover_preview(g_engine, dir);
            }
        }
        respond_json(fd, "{\"ok\":true}");
        return;
    }
    if (strcmp(req->path, "/api/favorites") == 0) {
        char q1[512], q2[512];
        snprintf(q1, sizeof(q1), "%s", req->query);
        snprintf(q2, sizeof(q2), "%s", req->query);
        char *name = query_get(q1, "name");
        char *on = query_get(q2, "on");
        if (name && on) {
            toggle_favorite(name, atoi(on));
            config_save(&g_cfg, g_config_path);
            respond_json(fd, "{\"ok\":true}");
            return;
        }
        http_respond(fd, "400 Bad Request", "text/plain", "missing args", 12);
        return;
    }
    if (strcmp(req->path, "/api/toggle-mute") == 0) {
        audio_engine_set_muted(g_engine, !audio_engine_get_muted(g_engine));
        respond_json(fd, audio_engine_get_muted(g_engine) ? "{\"muted\":true}" : "{\"muted\":false}");
        return;
    }
    if (strcmp(req->path, "/api/sample") == 0) {
        char q1[512], q2[512];
        snprintf(q1, sizeof(q1), "%s", req->query);
        snprintf(q2, sizeof(q2), "%s", req->query);
        char *name = query_get(q1, "name");
        char *file = query_get(q2, "file");
        if (name && file && !strstr(name, "..") && !strstr(file, "..")) {
            char path[900];
            snprintf(path, sizeof(path), "%s/%s/%s", g_cfg.sounds_dir, name, file);
            FILE *f = fopen(path, "rb");
            if (f) {
                fseek(f, 0, SEEK_END);
                long len = ftell(f);
                fseek(f, 0, SEEK_SET);
                char *buf = malloc((size_t)len);
                if (fread(buf, 1, (size_t)len, f) == (size_t)len)
                    http_respond(fd, "200 OK", "audio/wav", buf, (size_t)len);
                free(buf);
                fclose(f);
                return;
            }
        }
        http_respond(fd, "404 Not Found", "text/plain", "nope", 4);
        return;
    }
    http_respond(fd, "404 Not Found", "text/plain", "not found", 9);
}

static void on_key(InputEvent ev, void *userdata)
{
    AudioEngine *engine = userdata;

    /* mouse buttons -> optional click sounds (Keeby's MouseHook) */
    if (ev.code == BTN_LEFT || ev.code == BTN_RIGHT || ev.code == BTN_MIDDLE) {
        if (g_cfg.mouse_clicks)
            audio_engine_play_mouse(engine, ev.phase, ev.phase == 0 ? 0.55f : 0.40f);
        return;
    }

    const KeyPos *pos = keymap_lookup(ev.code);
    KeyPos dflt = KEYMAP_DEFAULT;
    if (!pos) pos = &dflt;

    /* enter overlay (Keeby PlayEnterOverlay) */
    if (ev.phase == 0 && pos->group == GRP_ENTER && g_cfg.enter_sound[0])
        audio_engine_play_enter_overlay(g_engine, g_cfg.enter_sound, g_cfg.enter_volume);

    /* UI visualizer stream */
    if (g_sse_fd >= 0) {
        char msg[96];
        int n = snprintf(msg, sizeof(msg), "data: {\"code\":%u,\"phase\":%d}\n\n",
                         (unsigned)ev.code, ev.phase);
        (void)!write(g_sse_fd, msg, n);
    }

    if (getenv("KEEBY_VERBOSE"))
        fprintf(stderr, "keebyd: key %u %s -> %s pan=%+.2f feel=%.2f\n",
                ev.code, ev.phase == 0 ? "down" : "up",
                keymap_group_name(pos->group), pos->pan, pos->feel);
    audio_engine_play(engine, pos->group, ev.phase, pos->pan, pos->feel);
}

static void on_hotkey(void *userdata)
{
    AudioEngine *engine = userdata;
    audio_engine_set_muted(engine, !audio_engine_get_muted(engine));
    klog("%s (hotkey)\n", audio_engine_get_muted(engine) ? "muted" : "unmuted");
}

static void list_profiles(const Config *cfg)
{
    char path[640];
    snprintf(path, sizeof(path), "%s", cfg->sounds_dir);
    printf("profiles in %s:\n", path);
    struct stat st;
    if (stat(path, &st) != 0) {
        printf("  (directory missing — run tools/synth_profiles.py)\n");
        return;
    }
    /* minimal listing via ls-like scan */
    char cmd[800];
    snprintf(cmd, sizeof(cmd), "ls -1 '%s' 2>/dev/null", path);
    int rc = system(cmd);
    (void)rc;
}

static void list_input_devices(void)
{
    printf("keyboards under /dev/input (per keebyd classification):\n");
    int rc = system(
        "for d in /dev/input/event*; do "
        "name=$(cat $d/device/name 2>/dev/null); "
        "phys=$(readlink -f $d/device 2>/dev/null); "
        "printf '  %-22s %s\\n' \"$d\" \"$name\"; done");
    (void)rc;
}

int main(int argc, char **argv)
{
    const char *config_path = NULL;
    const char *profile_override = NULL;
    const char *render_path = NULL;
    bool do_list = false, do_preview = false, do_devices = false;

    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--config") == 0 && i + 1 < argc) config_path = argv[++i];
        else if (strcmp(argv[i], "--profile") == 0 && i + 1 < argc) profile_override = argv[++i];
        else if (strcmp(argv[i], "--list") == 0) do_list = true;
        else if (strcmp(argv[i], "--devices") == 0) do_devices = true;
        else if (strcmp(argv[i], "--preview") == 0) do_preview = true;
        else if (strcmp(argv[i], "--render") == 0 && i + 1 < argc) render_path = argv[++i];
        else if (strcmp(argv[i], "--help") == 0 || strcmp(argv[i], "-h") == 0) {
            printf("usage: keebyd [--config PATH] [--profile NAME] [--preview] [--list] [--devices] [--render OUT.wav]\n"
                   "signals: SIGUSR1 toggle mute, SIGHUP reload config+profile\n");
            return 0;
        }
        else {
            fprintf(stderr, "unknown flag: %s\n", argv[i]);
            return 1;
        }
    }

    config_defaults(&g_cfg);
    if (!config_path) {
        const char *xdg = getenv("XDG_CONFIG_HOME");
        static char buf[512];
        if (xdg && *xdg) snprintf(buf, sizeof(buf), "%s/keebyd/config.conf", xdg);
        else snprintf(buf, sizeof(buf), "%s/.config/keebyd/config.conf", getenv("HOME") ? getenv("HOME") : "/tmp");
        config_path = buf;
    }
    snprintf(g_config_path, sizeof(g_config_path), "%s", config_path);
    if (config_load(&g_cfg, config_path) == 0)
        klog("config %s\n", config_path);
    else
        klog("no config at %s — using defaults\n", config_path);
    if (profile_override) snprintf(g_cfg.profile, sizeof(g_cfg.profile), "%s", profile_override);

    if (do_list) { list_profiles(&g_cfg); return 0; }
    if (do_devices) { list_input_devices(); return 0; }

    AudioSettings as = {
        .spatial_audio = g_cfg.spatial_audio,
        .per_key_feel = g_cfg.per_key_feel,
        .volume_normalization = g_cfg.volume_normalization,
        .mute_modifiers = g_cfg.mute_modifiers,
        .home_row_softness = g_cfg.home_row_softness,
        .master_volume = g_cfg.master_volume,
        .tone_lpf = g_cfg.tone_lpf,
        .tone_pitch = g_cfg.tone_pitch,
        .enabled = g_cfg.enabled,
        .muted = false,
    };
    g_engine = audio_engine_create(&as);
    install_profile(g_engine, &g_cfg);

    if (render_path) {
        render_benchmark(render_path, g_cfg.master_volume, audio_engine_normalization(g_engine));
        klog("rendered benchmark pattern -> %s", render_path);
        audio_engine_destroy(g_engine);
        return 0;
    }

    if (do_preview) {
        audio_engine_preview(g_engine);
        struct timespec ts = { 1, 0 };
        nanosleep(&ts, NULL); /* let the last samples drain */
        audio_engine_destroy(g_engine);
        return 0;
    }

    if (g_cfg.ui_port > 0)
        http_server_start(g_cfg.ui_port, route);

    HotkeyConfig hk = {
        .hotkey_key = g_cfg.hotkey_key,
        .hotkey_taps = g_cfg.hotkey_taps,
        .hotkey_ctrl = g_cfg.hotkey_ctrl,
    };
    g_monitor = input_monitor_create(&hk, on_key, on_hotkey, g_engine);

    signal(SIGINT, on_signal);
    signal(SIGTERM, on_signal);
    signal(SIGUSR1, on_usr1);
    signal(SIGHUP, on_hup);
    signal(SIGPIPE, SIG_IGN);

    fprintf(stderr, "keebyd: running — Ctrl+K x%d toggles mute, SIGUSR1 also toggles, SIGHUP reloads\n",
            hk.hotkey_taps);

    for (;;) {
        input_monitor_run(g_monitor);

        if (atomic_exchange(&g_reload, 0)) {
            klog("reloading config\n");
            Config fresh;
            config_defaults(&fresh);
            config_load(&fresh, config_path);
            if (profile_override) snprintf(fresh.profile, sizeof(fresh.profile), "%s", profile_override);
            g_cfg = fresh;
            AudioSettings as2 = {
                .spatial_audio = fresh.spatial_audio,
                .per_key_feel = fresh.per_key_feel,
                .volume_normalization = fresh.volume_normalization,
                .mute_modifiers = fresh.mute_modifiers,
                .home_row_softness = fresh.home_row_softness,
                .master_volume = fresh.master_volume,
                .tone_lpf = fresh.tone_lpf,
                .tone_pitch = fresh.tone_pitch,
                .enabled = fresh.enabled,
                .muted = false,
            };
            audio_engine_apply_settings(g_engine, &as2);
            install_profile(g_engine, &g_cfg);
            continue;
        }
        if (atomic_exchange(&g_toggle_mute, 0)) {
            audio_engine_set_muted(g_engine, !audio_engine_get_muted(g_engine));
            klog("%s (SIGUSR1)\n", audio_engine_get_muted(g_engine) ? "muted" : "unmuted");
            continue;
        }
        break; /* real stop */
    }

    klog("exiting\n");
    input_monitor_destroy(g_monitor);
    audio_engine_destroy(g_engine);
    return 0;
}
