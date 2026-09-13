/*
 * config.h — keebyd settings (mirrors Keeby's AppSettings defaults where
 * meaningful; ToneX/ToneY collapse into tone_lpf/tone_pitch here).
 */
#ifndef KEEBY_CONFIG_H
#define KEEBY_CONFIG_H

#include <stdbool.h>

#include "audio.h"

typedef struct {
    char   profile[128];
    char   sounds_dir[512];

    /* AudioSettings */
    float  master_volume;
    bool   enabled;
    bool   spatial_audio;
    bool   per_key_feel;
    float  home_row_softness;
    bool   volume_normalization;
    bool   mute_modifiers;
    float  tone_lpf;
    float  tone_pitch;

    /* extras */
    bool   mouse_clicks;
    int    hotkey_key;   /* linux keycode for the triple-tap toggle */
    int    hotkey_taps;
    bool   hotkey_ctrl;

    /* switch-picker UI (Keeby parity) */
    char   favorites[512];   /* comma-separated profile names */
    bool   hover_preview;    /* SwitchHoverPreview */
    char   enter_sound[128]; /* "", typewriter-enter.mp3, faahh-enter.mp3 */
    float  enter_volume;
    int    ui_port;          /* 0 = UI off */
} Config;

void config_defaults(Config *c);
/* Returns 0 if file was loaded (missing file is fine: defaults stay). */
int  config_load(Config *c, const char *path);
void config_save(const Config *c, const char *path);
void config_print(const Config *c);

#endif
