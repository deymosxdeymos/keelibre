/*
 * sound.c — sample decoding and profile loading.
 *
 * Mirrors Keeby's CachedSound.Decode(): stereo -> mono (0.5/0.5 mix),
 * resample to 44.1 kHz (cubic Hermite), keep as float[]. Files are decoded
 * with miniaudio's decoder, so wav/mp3/flac all work — that covers Keeby's
 * loose mp3 overlay sounds as well as its wav packs.
 */
#include "sound.h"

#include <ctype.h>
#include <dirent.h>
#include <errno.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <miniaudio.h>

#define MAX_SAMPLE_LEN (KEEBY_SAMPLE_RATE * 3) /* 3 s sanity cap */



/* ---------- resampling (cubic Hermite) ---------- */

static float cubic(float a, float b, float c, float d, float t)
{
    float a1 = 0.5f * (c - a);
    float a2 = a - 2.5f * b + 2.0f * c - 0.5f * d;
    float a3 = 0.5f * (d - a) + 1.5f * (b - c);
    return ((a3 * t + a2) * t + a1) * t + b;
}

static int resample_to_44k(const float *in, size_t in_len, unsigned in_rate,
                           float **out, size_t *out_len)
{
    if (in_rate == KEEBY_SAMPLE_RATE || in_len == 0) {
        float *buf = malloc(in_len * sizeof(float));
        if (!buf) return -1;
        memcpy(buf, in, in_len * sizeof(float));
        *out = buf;
        *out_len = in_len;
        return 0;
    }
    double step = (double)in_rate / KEEBY_SAMPLE_RATE;
    size_t n = (size_t)((double)in_len / step);
    if (n > MAX_SAMPLE_LEN) n = MAX_SAMPLE_LEN;
    float *buf = malloc((n ? n : 1) * sizeof(float));
    if (!buf) return -1;
    for (size_t i = 0; i < n; i++) {
        double src = (double)i * step;
        size_t idx = (size_t)src;
        float t = (float)(src - (double)idx);
        size_t i0 = idx > 0 ? idx - 1 : 0;
        size_t i1 = idx;
        size_t i2 = idx + 1 < in_len ? idx + 1 : in_len - 1;
        size_t i3 = idx + 2 < in_len ? idx + 2 : in_len - 1;
        buf[i] = cubic(in[i0], in[i1], in[i2], in[i3], t);
    }
    *out = buf;
    *out_len = n;
    return 0;
}

/* ---------- decode any format via miniaudio ---------- */

int decode_file(const char *path, float **out, size_t *out_len)
{
    ma_decoder_config dc = ma_decoder_config_init(ma_format_f32, 0, 0); /* native rate/chans */
    ma_decoder dec;
    if (ma_decoder_init_file(path, &dc, &dec) != MA_SUCCESS) {
        fprintf(stderr, "keebyd: %s: unsupported audio file\n", path);
        return -1;
    }
    ma_uint64 frames = 0;
    ma_result rc = ma_decoder_get_length_in_pcm_frames(&dec, &frames);
    size_t cap;
    if (rc == MA_SUCCESS && frames > 0)
        cap = (size_t)frames * dec.outputChannels + 4096;
    else
        cap = 44100 * 3 * (size_t)dec.outputChannels; /* worst case sanity */
    if (cap > MAX_SAMPLE_LEN * 2u * dec.outputChannels)
        cap = MAX_SAMPLE_LEN * 2u * dec.outputChannels;

    float *raw = malloc(cap * sizeof(float));
    if (!raw) { ma_decoder_uninit(&dec); return -1; }

    size_t total = 0;
    for (;;) {
        ma_uint64 got = 0;
        size_t chunk = cap - total;
        if (chunk == 0) break;
        rc = ma_decoder_read_pcm_frames(&dec, raw + total, chunk / dec.outputChannels, &got);
        if (rc != MA_SUCCESS && got == 0) break;
        total += (size_t)got * dec.outputChannels;
        if (rc != MA_SUCCESS) break;
        if (total == cap) break;
    }
    unsigned chans = dec.outputChannels;
    unsigned rate = dec.outputSampleRate;
    ma_decoder_uninit(&dec);
    if (total == 0) { free(raw); return -1; }

    /* mixdown to mono (0.5/0.5 weights like NAudio StereoToMono, generalized) */
    size_t nframes = total / chans;
    float *mono = malloc(nframes * sizeof(float));
    if (!mono) { free(raw); return -1; }
    double w = 1.0 / chans;
    for (size_t i = 0; i < nframes; i++) {
        double acc = 0.0;
        for (unsigned c = 0; c < chans; c++) acc += raw[i * chans + c] * w;
        mono[i] = (float)(acc * 2.0 * w * chans * 0.5);
    }
    free(raw);

    int rrc = resample_to_44k(mono, nframes, rate, out, out_len);
    free(mono);
    return rrc;
}

/* keep the public name used elsewhere */
int wav_decode_file(const char *path, float **out, size_t *out_len)
{
    return decode_file(path, out, out_len);
}

/* ---------- profile loading ---------- */

typedef struct {
    KeyGroup group;
    int      phase; /* 0 down, 1 up */
    char     path[600];
} PackFile;

static int parse_name(const char *fn, PackFile *pf)
{
    /* <group>_<phase>_<NN>.wav ; group may contain '_' (none today, but safe) */
    const char *dot = strrchr(fn, '.');
    if (!dot || strcasecmp(dot, ".wav") != 0) return 0;
    char stem[256];
    size_t n = (size_t)(dot - fn);
    if (n == 0 || n >= sizeof(stem)) return 0;
    memcpy(stem, fn, n);
    stem[n] = 0;

    char *us = strrchr(stem, '_'); /* _NN */
    if (!us) return 0;
    *us = 0;
    for (const char *q = us + 1; *q; q++) if (!isdigit((unsigned char)*q)) return 0;

    char *ph = strrchr(stem, '_'); /* _phase */
    if (!ph) return 0;
    *ph = 0;
    if (strcmp(ph + 1, "down") == 0) pf->phase = 0;
    else if (strcmp(ph + 1, "up") == 0) pf->phase = 1;
    else return 0;

    for (KeyGroup g = 0; g < GRP_COUNT; g++) {
        if (strcmp(stem, keymap_group_name(g)) == 0) {
            pf->group = g;
            return 1;
        }
    }
    return 0;
}

static int cmp_packfile(const void *a, const void *b)
{
    return strcmp(((const PackFile *)a)->path, ((const PackFile *)b)->path);
}

static void read_profile_conf(const char *dir, float *norm)
{
    char path[640];
    snprintf(path, sizeof(path), "%s/profile.conf", dir);
    FILE *f = fopen(path, "r");
    if (!f) return;
    char line[256];
    while (fgets(line, sizeof(line), f)) {
        char *h = line;
        while (isspace((unsigned char)*h)) h++;
        if (*h == '#' || *h == ';' || *h == 0) continue;
        char key[64];
        float val;
        if (sscanf(h, "%63s = %f", key, &val) == 2) {
            if (strcmp(key, "normalization_gain") == 0) *norm = val;
        }
    }
    fclose(f);
}

int profile_load(Profile *prof, const char *dir)
{
    memset(prof, 0, sizeof(*prof));
    prof->normalization_gain = 1.0f;

    const char *base = strrchr(dir, '/');
    snprintf(prof->name, sizeof(prof->name), "%s", base ? base + 1 : dir);
    snprintf(prof->dir, sizeof(prof->dir), "%s", dir);
    read_profile_conf(dir, &prof->normalization_gain);

    DIR *d = opendir(dir);
    if (!d) {
        fprintf(stderr, "keebyd: profile dir %s: %s\n", dir, strerror(errno));
        return -1;
    }
    PackFile files[256];
    int nf = 0;
    struct dirent *de;
    while ((de = readdir(d)) && nf < 256) {
        PackFile pf;
        if (!parse_name(de->d_name, &pf)) continue;
        snprintf(pf.path, sizeof(pf.path), "%s/%s", dir, de->d_name);
        files[nf++] = pf;
    }
    closedir(d);
    qsort(files, nf, sizeof(files[0]), cmp_packfile);

    int loaded = 0;
    for (int i = 0; i < nf; i++) {
        SampleSet *set = &prof->sets[files[i].group][files[i].phase];
        if (set->count >= MAX_VARIATIONS) continue;
        float *data;
        size_t len;
        if (decode_file(files[i].path, &data, &len) != 0) continue;
        Sample *s = &set->variations[set->count++];
        s->data = data;
        s->len = len;
        loaded++;
    }

    /* shared sounds: <sounds_root>/_shared/mouse_*.wav — Keeby keeps its mouse
     * sounds loose in Sounds/ and addresses them via MouseSoundCatalog; a
     * _shared dir is the pack-system equivalent. */
    if (prof->sets[GRP_MOUSE][0].count == 0) {
        char shared[700];
        snprintf(shared, sizeof(shared), "%.500s/_shared", dir);
        DIR *sd = opendir(shared);
        if (sd) {
            struct dirent *se;
            while ((se = readdir(sd)) && prof->sets[GRP_MOUSE][0].count < MAX_VARIATIONS) {
                PackFile pf;
                if (!parse_name(se->d_name, &pf) || pf.group != GRP_MOUSE) continue;
                char p2[760];
                snprintf(p2, sizeof(p2), "%s/%s", shared, se->d_name);
                SampleSet *set = &prof->sets[GRP_MOUSE][pf.phase];
                if (set->count >= MAX_VARIATIONS) continue;
                float *data;
                size_t len;
                if (decode_file(p2, &data, &len) != 0) continue;
                Sample *s = &set->variations[set->count++];
                s->data = data;
                s->len = len;
                loaded++;
            }
            closedir(sd);
        }
    }

    fprintf(stderr, "keebyd: profile '%s': %d samples, normalization_gain=%.2f\n",
            prof->name, loaded, prof->normalization_gain);
    return loaded > 0 ? 0 : -1;
}

void profile_free(Profile *prof)
{
    if (!prof) return;
    for (int g = 0; g < GRP_COUNT; g++)
        for (int p = 0; p < 2; p++)
            for (int v = 0; v < prof->sets[g][p].count; v++)
                free(prof->sets[g][p].variations[v].data);
    memset(prof, 0, sizeof(*prof));
}
