/*
 * config.c — tiny key = value parser, no dependencies.
 */
#include "config.h"

#include <ctype.h>
#include <linux/input-event-codes.h>
#include <stdio.h>
#include <string.h>

#include <pwd.h>
#include <stdlib.h>
#include <sys/types.h>
#include <unistd.h>

void config_defaults(Config *c)
{
    memset(c, 0, sizeof(*c));
    snprintf(c->profile, sizeof(c->profile), "thocky-linear");
    c->sounds_dir[0] = 0;
    c->master_volume = 1.0f;
    c->enabled = true;
    c->spatial_audio = true;
    c->per_key_feel = true;
    c->home_row_softness = 1.0f;
    c->volume_normalization = true;
    c->mute_modifiers = false;
    c->tone_lpf = 0.5f;   /* Keeby ToneX default */
    c->tone_pitch = 1.0f; /* Keeby 0.88 + ToneY(0.5)*0.24 */
    c->mouse_clicks = false;
    c->hotkey_key = KEY_K;
    c->hotkey_taps = 3;
    c->hotkey_ctrl = true;
    snprintf(c->favorites, sizeof(c->favorites), "");
    c->hover_preview = true;   /* Keeby SwitchHoverPreview default */
    snprintf(c->enter_sound, sizeof(c->enter_sound), "");
    c->enter_volume = 1.0f;
    c->ui_port = 7777;
}

static void resolve_sounds_dir(Config *c)
{
    if (c->sounds_dir[0]) return;
    const char *xdg = getenv("XDG_DATA_HOME");
    char buf[512];
    if (xdg && *xdg) {
        snprintf(buf, sizeof(buf), "%s/keebyd/sounds", xdg);
    } else {
        const char *home = getenv("HOME");
        if (!home) {
            struct passwd *pw = getpwuid(getuid());
            home = pw ? pw->pw_dir : "/tmp";
        }
        snprintf(buf, sizeof(buf), "%s/.local/share/keebyd/sounds", home);
    }
    snprintf(c->sounds_dir, sizeof(c->sounds_dir), "%s", buf);
}

static bool parse_bool(const char *v, bool dflt)
{
    if (!*v) return dflt;
    return strcasecmp(v, "true") == 0 || strcasecmp(v, "yes") == 0 ||
           strcasecmp(v, "on") == 0 || strcmp(v, "1") == 0;
}

int config_load(Config *c, const char *path)
{
    FILE *f = fopen(path, "r");
    if (!f) return -1;
    char line[512];
    while (fgets(line, sizeof(line), f)) {
        char *h = line;
        while (isspace((unsigned char)*h)) h++;
        if (*h == '#' || *h == ';' || *h == '\n' || *h == 0) continue;
        char *eq = strchr(h, '=');
        if (!eq) continue;
        *eq = 0;
        char *k = h, *v = eq + 1;
        while (isspace((unsigned char)*k) || *k == 0) k++;
        char *ke = k + strlen(k);
        while (ke > k && isspace((unsigned char)ke[-1])) *--ke = 0;
        while (isspace((unsigned char)*v)) v++;
        char *ve = v + strlen(v);
        while (ve > v && isspace((unsigned char)ve[-1])) *--ve = 0;

        if (strcmp(k, "profile") == 0) snprintf(c->profile, sizeof(c->profile), "%s", v);
        else if (strcmp(k, "sounds_dir") == 0) snprintf(c->sounds_dir, sizeof(c->sounds_dir), "%s", v);
        else if (strcmp(k, "master_volume") == 0) c->master_volume = strtof(v, NULL);
        else if (strcmp(k, "enabled") == 0) c->enabled = parse_bool(v, c->enabled);
        else if (strcmp(k, "spatial_audio") == 0) c->spatial_audio = parse_bool(v, c->spatial_audio);
        else if (strcmp(k, "per_key_feel") == 0) c->per_key_feel = parse_bool(v, c->per_key_feel);
        else if (strcmp(k, "home_row_softness") == 0) c->home_row_softness = strtof(v, NULL);
        else if (strcmp(k, "volume_normalization") == 0) c->volume_normalization = parse_bool(v, c->volume_normalization);
        else if (strcmp(k, "mute_modifiers") == 0) c->mute_modifiers = parse_bool(v, c->mute_modifiers);
        else if (strcmp(k, "tone_lpf") == 0) c->tone_lpf = strtof(v, NULL);
        else if (strcmp(k, "tone_pitch") == 0) c->tone_pitch = strtof(v, NULL);
        else if (strcmp(k, "mouse_clicks") == 0) c->mouse_clicks = parse_bool(v, c->mouse_clicks);
        else if (strcmp(k, "hotkey_key") == 0) c->hotkey_key = atoi(v);
        else if (strcmp(k, "hotkey_taps") == 0) c->hotkey_taps = atoi(v);
        else if (strcmp(k, "hotkey_ctrl") == 0) c->hotkey_ctrl = parse_bool(v, c->hotkey_ctrl);
        else if (strcmp(k, "favorites") == 0) snprintf(c->favorites, sizeof(c->favorites), "%s", v);
        else if (strcmp(k, "hover_preview") == 0) c->hover_preview = parse_bool(v, c->hover_preview);
        else if (strcmp(k, "enter_sound") == 0) snprintf(c->enter_sound, sizeof(c->enter_sound), "%s", v);
        else if (strcmp(k, "enter_volume") == 0) c->enter_volume = strtof(v, NULL);
        else if (strcmp(k, "ui_port") == 0) c->ui_port = atoi(v);
    }
    fclose(f);
    resolve_sounds_dir(c);
    return 0;
}

void config_save(const Config *c, const char *path)
{
    FILE *f = fopen(path, "w");
    if (!f) return;
    fprintf(f, "# keebyd configuration\n");
    fprintf(f, "profile = %s\n", c->profile);
    fprintf(f, "sounds_dir = %s\n", c->sounds_dir);
    fprintf(f, "master_volume = %.2f\n", c->master_volume);
    fprintf(f, "enabled = %s\n", c->enabled ? "true" : "false");
    fprintf(f, "spatial_audio = %s\n", c->spatial_audio ? "true" : "false");
    fprintf(f, "per_key_feel = %s\n", c->per_key_feel ? "true" : "false");
    fprintf(f, "home_row_softness = %.2f\n", c->home_row_softness);
    fprintf(f, "volume_normalization = %s\n", c->volume_normalization ? "true" : "false");
    fprintf(f, "mute_modifiers = %s\n", c->mute_modifiers ? "true" : "false");
    fprintf(f, "tone_lpf = %.2f\n", c->tone_lpf);
    fprintf(f, "tone_pitch = %.2f\n", c->tone_pitch);
    fprintf(f, "mouse_clicks = %s\n", c->mouse_clicks ? "true" : "false");
    fprintf(f, "hotkey_key = %d\n", c->hotkey_key);
    fprintf(f, "hotkey_taps = %d\n", c->hotkey_taps);
    fprintf(f, "hotkey_ctrl = %s\n", c->hotkey_ctrl ? "true" : "false");
    fprintf(f, "favorites = %s\n", c->favorites);
    fprintf(f, "hover_preview = %s\n", c->hover_preview ? "true" : "false");
    fprintf(f, "enter_sound = %s\n", c->enter_sound);
    fprintf(f, "enter_volume = %.2f\n", c->enter_volume);
    fprintf(f, "ui_port = %d\n", c->ui_port);
    fclose(f);
}

void config_print(const Config *c)
{
    fprintf(stderr, "keebyd: profile=%s sounds_dir=%s master=%.2f tone(lpf=%.2f pitch=%.2f) "
                    "spatial=%d feel=%d(hrs=%.2f) norm=%d mute_mod=%d mouse=%d\n",
            c->profile, c->sounds_dir, c->master_volume, c->tone_lpf, c->tone_pitch,
            c->spatial_audio, c->per_key_feel, c->home_row_softness,
            c->volume_normalization, c->mute_modifiers, c->mouse_clicks);
}
