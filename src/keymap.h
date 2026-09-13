/*
 * keymap.h — Linux keycode -> (group, pan, feelGain)
 *
 * Port of Keeby's Resources/keymap.json ("Mirror of Mac KeyPositionMap.swift
 * but with VK codes"), translated from Windows Virtual-Key codes to Linux
 * input-event-codes. pan: -1.0 (left) .. +1.0 (right). feelGain: ergonomic
 * typing-dynamics multiplier (home row softer, stretch keys louder).
 */
#ifndef KEEBY_KEYMAP_H
#define KEEBY_KEYMAP_H

#include <linux/input-event-codes.h>

typedef enum {
    GRP_ALPHA = 0,
    GRP_SPACE,
    GRP_ENTER,
    GRP_BACKSPACE,
    GRP_MODIFIER,
    GRP_TAB,
    GRP_ARROW,
    GRP_MOUSE,
    GRP_COUNT
} KeyGroup;

typedef struct {
    KeyGroup group;
    float    pan;   /* -1..+1 */
    float    feel;  /* gain multiplier, 1.0 neutral */
} KeyPos;

/* Defaults for unmapped keys (Keeby: KeyPosition(0, Alpha, 1.0)) */
#define KEYMAP_DEFAULT ((KeyPos){ GRP_ALPHA, 0.0f, 1.0f })

const KeyPos *keymap_lookup(unsigned short code);
const char   *keymap_group_name(KeyGroup g);

#endif
