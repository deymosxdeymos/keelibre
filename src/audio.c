/*
 * audio.c — playback engine.
 *
 * Port of Keeby.Audio.AudioEngine + NAudio provider chain:
 *
 *   CachedSoundSampleProvider   -> per-voice pointer into cached mono f32
 *   PitchShiftSampleProvider    -> varispeed with linear interpolation
 *   ToneLpfSampleProvider       -> one-pole LPF, alpha=max(0.06, c*c),
 *                                  makeup = 1+(1-c)^2*3, dry = (1-c)*0.45
 *   PanningSampleProvider       -> equal-power stereo pan
 *   VolumeSampleProvider        -> clamp(feel * norm * master, 0, 4)
 *   MixingSampleProvider        -> our voice pool mixed in the device callback
 *
 * Trigger path is lock-free: voices are claimed with an atomic flag using
 * acquire/release ordering; the audio callback is the only mutator of voice
 * position after the claim.
 */
#include "audio.h"

#include <stdatomic.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <time.h>
#include <math.h>
#include <miniaudio.h>



#define VOICE_COUNT  64
#define DEVICE_RATE  KEEBY_SAMPLE_RATE

typedef struct {
    /* trigger thread writes fields while state==CLAIMED, then publishes with READY */
    _Atomic int  state; /* 0=FREE 1=READY 2=CLAIMED */
    const float *data;
    unsigned     len;
    float        rate;        /* varispeed: samples advanced per output sample */
    float        gain_l, gain_r;
    float        vol;
    float        lpf_alpha, lpf_makeup, lpf_dry;
    /* audio-thread owned after publication */
    unsigned     pos;
    float        src;         /* fractional read position */
    float        lpf_state;
} Voice;

struct AudioEngine {
    AudioSettings cfg;
    bool muted_runtime; /* hotkey/SIGUSR1 toggle, distinct from cfg.muted */

    ma_device device;
    bool      device_ok;

    Profile  *profile;
    atomic_uint rr[GRP_COUNT][2];
    atomic_uint mouse_rr[2];

    Voice voices[VOICE_COUNT];
    char shared_dir[512];
};

/* ---------- mixer core (shared by live callback and --render) ---------- */

static inline void mix_voice(Voice *v, float *out, unsigned frames)
{
    for (unsigned i = 0; i < frames; i++) {
        unsigned ipos = (unsigned)v->src;
        if (ipos + 1 >= v->len) {
            atomic_store_explicit(&v->state, 0, memory_order_release); /* FREE */
            return;
        }
        float t = v->src - (float)ipos;
        float s = v->data[ipos] * (1.0f - t) + v->data[ipos + 1] * t;

        if (v->lpf_alpha < 1.0f) {
            v->lpf_state += v->lpf_alpha * (s - v->lpf_state);
            s = v->lpf_state * v->lpf_makeup + s * v->lpf_dry;
        }
        out[2 * i] += s * v->gain_l * v->vol;     /* interleaved L */
        out[2 * i + 1] += s * v->gain_r * v->vol; /* interleaved R */

        v->src += v->rate;
        v->pos++;
    }
}

void keeby_engine_mix(AudioEngine *e, float *out, unsigned frames)
{
    for (int i = 0; i < VOICE_COUNT; i++) {
        Voice *v = &e->voices[i];
        if (atomic_load_explicit(&v->state, memory_order_acquire) != 1) continue; /* not READY */
        Voice vcopy;
        vcopy.data = v->data;
        vcopy.len = v->len;
        vcopy.rate = v->rate;
        vcopy.gain_l = v->gain_l;
        vcopy.gain_r = v->gain_r;
        vcopy.vol = v->vol;
        vcopy.lpf_alpha = v->lpf_alpha;
        vcopy.lpf_makeup = v->lpf_makeup;
        vcopy.lpf_dry = v->lpf_dry;
        vcopy.state = v->state;
        vcopy.pos = v->pos;
        vcopy.src = v->src;
        vcopy.lpf_state = v->lpf_state;
        mix_voice(&vcopy, out, frames);
        if (atomic_load_explicit(&vcopy.state, memory_order_relaxed) == 1) {
            /* still playing: write back audio-thread-owned cursor state */
            v->src = vcopy.src;
            v->pos = vcopy.pos;
            v->lpf_state = vcopy.lpf_state;
        } else {
            atomic_store_explicit(&v->state, 0, memory_order_release); /* FREE */
        }
    }

    /* Keeby's SoftLimiterSampleProvider: tanh above 0.9 keeps overlapping
     * keystrokes from hard-clipping (clipped transients are what makes
     * mechanical-keyboard output painful). */
    for (unsigned i = 0; i < frames; i++) {
        float l = out[2 * i], r = out[2 * i + 1];
        if (l > 0.9f || l < -0.9f) out[2 * i] = tanhf(l);
        if (r > 0.9f || r < -0.9f) out[2 * i + 1] = tanhf(r);
    }
}

static void data_callback(ma_device *dev, void *out_, const void *in_, ma_uint32 frames)
{
    (void)dev; (void)in_;
    AudioEngine *e = dev->pUserData;
    if (!e) return;

    float *out = (float *)out_;
    memset(out, 0, frames * 2 * sizeof(float));
    if (!e->cfg.enabled || e->muted_runtime) return;

    keeby_engine_mix(e, out, (unsigned)frames);
}

/* ---------- voice claiming ---------- */

static void claim_and_submit(AudioEngine *e, const Sample *s, float pan, float vol,
                             float lpf, float pitch)
{
    if (!s || s->len < 2 || !e->device_ok) return;

    Voice *v = NULL;
    for (int i = 0; i < VOICE_COUNT; i++) {
        int expected = 0; /* FREE */
        if (atomic_compare_exchange_strong(&e->voices[i].state, &expected, 2 /* CLAIMED */)) {
            v = &e->voices[i];
            break;
        }
    }
    if (!v) return; /* all voices busy: drop, like a full mixer */

    float c = lpf < 0.99f ? lpf : 1.0f;
    float alpha = c * c; if (alpha < 0.06f) alpha = 0.06f;
    float makeup = 1.0f + (1.0f - c) * (1.0f - c) * 3.0f;
    float dry = (1.0f - c) * 0.45f;

    float th = (pan + 1.0f) * 0.78539816339f; /* (pan+1)*pi/4 */
    float gl = cosf(th), gr = sinf(th);

    v->data = s->data;
    v->len = (unsigned)s->len;
    v->rate = pitch;
    v->gain_l = gl;
    v->gain_r = gr;
    v->vol = vol < 0.0f ? 0.0f : (vol > 4.0f ? 4.0f : vol);
    v->lpf_alpha = c < 0.99f ? alpha : 1.0f;
    v->lpf_makeup = makeup;
    v->lpf_dry = dry;
    v->src = 0.0f;
    v->pos = 0;
    v->lpf_state = 0.0f;
    atomic_store_explicit(&v->state, 1, memory_order_release); /* publish READY */
}

/* ---------- public API ---------- */

AudioEngine *audio_engine_create(const AudioSettings *cfg)
{
    AudioEngine *e = calloc(1, sizeof(*e));
    if (!e) return NULL;
    if (cfg) e->cfg = *cfg;

    ma_device_config dc = ma_device_config_init(ma_device_type_playback);
    dc.playback.format = ma_format_f32;
    dc.playback.channels = 2;
    dc.sampleRate = DEVICE_RATE;
    dc.periodSizeInFrames = 256; /* ~5.8 ms — Keeby runs 80 ms WASAPI; we can go lower */
    dc.dataCallback = data_callback;
    dc.pUserData = e;

    if (ma_device_init(NULL, &dc, &e->device) != MA_SUCCESS) {
        fprintf(stderr, "keebyd: no audio device available — running silent\n");
        return e; /* engine still functional; submits are no-ops */
    }
    if (ma_device_start(&e->device) != MA_SUCCESS) {
        fprintf(stderr, "keebyd: failed to start audio output\n");
        ma_device_uninit(&e->device);
        return e;
    }
    e->device_ok = true;
    fprintf(stderr, "keebyd: audio out: %s backend @ %u Hz, period %u frames\n",
            ma_get_backend_name(e->device.pContext->backend),
            e->device.sampleRate, e->device.playback.internalPeriodSizeInFrames);
    return e;
}

void audio_engine_destroy(AudioEngine *engine)
{
    if (!engine) return;
    if (engine->device_ok) ma_device_uninit(&engine->device);
    profile_free(engine->profile);
    free(engine->profile);
    free(engine);
}

void audio_engine_set_profile(AudioEngine *engine, Profile *prof)
{
    profile_free(engine->profile);
    free(engine->profile);
    engine->profile = prof;
    for (int g = 0; g < GRP_COUNT; g++)
        for (int p = 0; p < 2; p++)
            atomic_store(&engine->rr[g][p], 0u);
}

void audio_engine_set_muted(AudioEngine *engine, bool muted)
{
    engine->muted_runtime = muted;
}

bool audio_engine_get_muted(const AudioEngine *engine)
{
    return engine->muted_runtime;
}

float audio_engine_normalization(const AudioEngine *engine)
{
    return engine->profile ? engine->profile->normalization_gain : 1.0f;
}

void audio_engine_apply_settings(AudioEngine *engine, const AudioSettings *settings)
{
    engine->cfg = *settings; /* muted is runtime-only, lives outside cfg */
}

static const Sample *pick_variation(AudioEngine *e, KeyGroup group, int phase)
{
    Profile *p = e->profile;
    if (!p) return NULL;
    SampleSet *set = &p->sets[group][phase];
    if (set->count == 0) return NULL;
    unsigned idx = atomic_fetch_add(&e->rr[group][phase], 1u) % (unsigned)set->count;
    return &set->variations[idx];
}

void audio_engine_play(AudioEngine *e, KeyGroup group, int phase, float pan, float feel)
{
    if (!e->cfg.enabled || e->muted_runtime) return;
    if (e->cfg.mute_modifiers && group == GRP_MODIFIER) return;

    const Sample *s = pick_variation(e, group, phase);
    if (!s) s = pick_variation(e, GRP_ALPHA, phase); /* Keeby's alpha fallback */
    if (!s) return;

    float f = feel;
    if (e->cfg.per_key_feel) {
        if (f < 1.0f) f = 1.0f + (f - 1.0f) * e->cfg.home_row_softness;
    } else {
        f = 1.0f;
    }
    float norm = e->cfg.volume_normalization && e->profile ? e->profile->normalization_gain : 1.0f;
    float vol = f * norm * e->cfg.master_volume;
    float pan2 = e->cfg.spatial_audio ? pan : 0.0f;
    claim_and_submit(e, s, pan2, vol, e->cfg.tone_lpf, e->cfg.tone_pitch);
}

void audio_engine_play_mouse(AudioEngine *e, int phase, float gain)
{
    if (!e->cfg.enabled || audio_engine_get_muted(e) || !e->profile) return;
    SampleSet *set = &e->profile->sets[GRP_MOUSE][phase ? 1 : 0];
    if (set->count == 0) return;
    unsigned idx = atomic_fetch_add(&e->mouse_rr[phase ? 1 : 0], 1u) % (unsigned)set->count;
    float vol = gain * e->cfg.master_volume;
    claim_and_submit(e, &set->variations[idx], 0.0f, vol, e->cfg.tone_lpf, e->cfg.tone_pitch);
}

void audio_engine_preview(AudioEngine *e)
{
    if (!e->profile) return;
    /* Keeby PlaySwitchPreviewBurst: 3 strokes, 180 ms apart, up ~85 ms after down */
    for (int i = 0; i < 3; i++) {
        audio_engine_play(e, GRP_ALPHA, 0, 0.0f, 1.0f);
        struct timespec ts = { .tv_sec = 0, .tv_nsec = 85 * 1000 * 1000 };
        nanosleep(&ts, NULL);
        audio_engine_play(e, GRP_ALPHA, 1, 0.0f, 1.0f);
        ts.tv_nsec = 95 * 1000 * 1000;
        nanosleep(&ts, NULL);
    }
}

void audio_engine_hover_preview(AudioEngine *e, const char *profile_dir)
{
    /* one-slot cache like Keeby's _previewCache */
    static Profile *cached = NULL;
    static char cached_dir[512] = { 0 };
    static AudioEngine *cached_owner = NULL;
    if (!cached || cached_owner != e || strcmp(cached_dir, profile_dir) != 0) {
        profile_free(cached);
        free(cached);
        cached = malloc(sizeof(Profile));
        if (!cached) return;
        if (profile_load(cached, profile_dir) != 0) {
            free(cached);
            cached = NULL;
            return;
        }
        snprintf(cached_dir, sizeof(cached_dir), "%s", profile_dir);
        cached_owner = e;
    }
    SampleSet *set = &cached->sets[GRP_ALPHA][0];
    if (set->count == 0) return;
    float pan = (float)(((double)rand() / RAND_MAX) - 0.5) * 0.4f;
    float vol = e->cfg.master_volume;
    claim_and_submit(e, &set->variations[0], pan, vol, e->cfg.tone_lpf, e->cfg.tone_pitch);
}

void audio_engine_set_shared_dir(AudioEngine *e, const char *dir)
{
    snprintf(e->shared_dir, sizeof(e->shared_dir), "%s", dir ? dir : "");
}

void audio_engine_play_enter_overlay(AudioEngine *e, const char *filename, float volume)
{
    if (e->muted_runtime || !filename || !filename[0] || !e->shared_dir[0]) return;

    /* tiny filename-keyed cache (Keeby _enterSounds) */
    static struct { char name[128]; Sample sample; } cache[4];
    static int ncached = 0;
    Sample *smp = NULL;
    for (int i = 0; i < ncached; i++)
        if (strcmp(cache[i].name, filename) == 0) { smp = &cache[i].sample; break; }
    if (!smp && ncached < 4) {
        char path[768];
        snprintf(path, sizeof(path), "%s/%s", e->shared_dir, filename);
        float *data;
        size_t len;
        if (decode_file(path, &data, &len) == 0) {
            snprintf(cache[ncached].name, sizeof(cache[ncached].name), "%s", filename);
            cache[ncached].sample.data = data;
            cache[ncached].sample.len = len;
            smp = &cache[ncached].sample;
            ncached++;
        } else {
            return;
        }
    }
    if (!smp) return;
    float vol = volume * e->cfg.master_volume;
    claim_and_submit(e, smp, 0.0f, vol, e->cfg.tone_lpf, e->cfg.tone_pitch);
}
