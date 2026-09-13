/*
 * catalog.h — port of Keeby.Audio.SwitchCatalog: the switch picker metadata
 * (display name, brand, type, force, color, contributor, normalization gain).
 */
#ifndef KEEBY_CATALOG_H
#define KEEBY_CATALOG_H

typedef struct {
    const char *dir;      /* pack directory name */
    const char *display;  /* human name */
    const char *brand;
    const char *type;     /* "Linear · 45g" etc. */
    const char *color;    /* hex, picker dot */
    const char *contributor; /* "" or "Name (@handle)" */
    float       norm;
} SwitchMeta;

/* 22 switches from the decompiled v1.8.1 SwitchCatalog */
extern const SwitchMeta CATALOG[];
extern const int CATALOG_LEN;

const SwitchMeta *catalog_find(const char *dir);

#endif
