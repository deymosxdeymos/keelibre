/*
 * sound.h — sample loading and switch-profile management.
 *
 * A "profile" is a directory of WAV files named
 *     <group>_<phase>_<NN>.wav
 * exactly like Keeby's Resources/Sounds/<switch>/ layout, plus an optional
 * profile.conf with `normalization_gain = <float>` (Keeby's per-profile
 * SwitchCatalog.NormalizationGain).
 */
#ifndef KEEBY_SOUND_H
#define KEEBY_SOUND_H

#include <stdbool.h>
#include <stddef.h>

#include "keymap.h"

#define KEEBY_SAMPLE_RATE 44100
#define MAX_VARIATIONS     8

typedef struct {
    float *data;        /* mono f32 @ KEEBY_SAMPLE_RATE */
    size_t len;
} Sample;

typedef struct {
    Sample variations[MAX_VARIATIONS];
    int    count;
} SampleSet;

typedef struct {
    char       name[128];
    char       dir[512];
    SampleSet  sets[GRP_COUNT][2]; /* [group][phase 0=down 1=up] */
    float      normalization_gain;
} Profile;

/* Load all WAVs from dir into prof. Returns 0 on success. */
int  profile_load(Profile *prof, const char *dir);

/* Free all sample memory. */
void profile_free(Profile *prof);

/* Decode any audio file (wav/mp3/flac via miniaudio) to mono f32 @ 44100. */
int  wav_decode_file(const char *path, float **out, size_t *out_len);
int  decode_file(const char *path, float **out, size_t *out_len);

#endif
