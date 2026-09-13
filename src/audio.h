/*
 * audio.h — playback engine (miniaudio) ported from Keeby.Audio.AudioEngine.
 */
#ifndef KEEBY_AUDIO_H
#define KEEBY_AUDIO_H

#include <stdbool.h>
#include <stdint.h>

#include "sound.h"

typedef struct {
    bool  spatial_audio;      /* pan by physical key position        */
    bool  per_key_feel;       /* feelGain (home row softer)          */
    bool  volume_normalization;
    bool  mute_modifiers;
    float home_row_softness;  /* scales feelGain below 1.0           */
    float master_volume;
    float tone_lpf;           /* 0..1, Keeby ToneX                   */
    float tone_pitch;         /* 0.5..2, Keeby 0.88 + ToneY*0.24     */

    /* runtime */
    bool enabled;
    bool muted;
} AudioSettings;

typedef struct AudioEngine AudioEngine;

AudioEngine *audio_engine_create(const AudioSettings *settings);
void         audio_engine_destroy(AudioEngine *engine);

void audio_engine_set_profile(AudioEngine *engine, Profile *prof); /* engine takes ownership */

void audio_engine_set_muted(AudioEngine *engine, bool muted);
bool audio_engine_get_muted(const AudioEngine *engine);
float audio_engine_normalization(const AudioEngine *engine);
/* Update DSP/settings fields live (profile untouched). */
void audio_engine_apply_settings(AudioEngine *engine, const AudioSettings *settings);

/* Trigger a key event. Phase: 0 = down, 1 = up. */
void audio_engine_play(AudioEngine *engine, KeyGroup group, int phase, float pan, float feel);
/* Mouse click (pan 0). */
void audio_engine_play_mouse(AudioEngine *engine, int phase, float gain);
/* Preview burst: 3 down/up strokes on the alpha set (Keeby's notch preview). */
void audio_engine_preview(AudioEngine *engine);

/* Keeby's PlayPreview: one alpha-down stroke from another profile dir with a
 * random pan in ±0.2 (used by hover-preview in the switch picker). */
void audio_engine_hover_preview(AudioEngine *engine, const char *profile_dir);

/* Enter-overlay sound (Keeby PlayEnterOverlay): filename inside the shared
 * sounds dir (mp3/wav). Only plays on Enter down. */
void audio_engine_set_shared_dir(AudioEngine *engine, const char *dir);
void audio_engine_play_enter_overlay(AudioEngine *engine, const char *filename, float volume);

/* Offline rendering for benchmarking: mix whatever is in the voice pool
 * into an interleaved-stereo buffer (identical to the live callback). */
void keeby_engine_mix(AudioEngine *engine, float *out, unsigned frames);

#endif
