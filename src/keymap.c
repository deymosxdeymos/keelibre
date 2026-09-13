/*
 * keymap.c — VK->Linux keycode port of Keeby's keymap.json.
 *
 * Original Windows VK table (pan / group / feelGain) preserved 1:1 for keys
 * that exist on Linux; numpad, ISO extra key and Japanese keys added with
 * sensible positions. See NOTES.md.
 */
#include "keymap.h"

#include <stddef.h>

typedef struct {
    unsigned short code;
    KeyPos         pos;
} KeyMapEntry;

static const KeyMapEntry g_map[] = {
    /* ---- Editing / special ---- */
    { KEY_BACKSPACE,        { GRP_BACKSPACE, +0.95f, 1.00f } }, /* VK 8  */
    { KEY_TAB,              { GRP_TAB,       -0.90f, 1.00f } }, /* VK 9  */
    { KEY_ENTER,            { GRP_ENTER,     +0.95f, 1.00f } }, /* VK 13 */
    { KEY_KPENTER,          { GRP_ENTER,     +0.95f, 1.00f } },
    { KEY_CAPSLOCK,         { GRP_MODIFIER,  -0.90f, 1.00f } }, /* VK 20 */
    { KEY_ESC,              { GRP_MODIFIER,  -0.95f, 1.00f } }, /* VK 27 */
    { KEY_SPACE,            { GRP_SPACE,      0.00f, 1.00f } }, /* VK 32 */
    { KEY_DELETE,           { GRP_BACKSPACE, +0.75f, 1.00f } }, /* site demo: Delete -> backspace */

    /* ---- Arrows (VK 37/38/39/40) ---- */
    { KEY_LEFT,             { GRP_ARROW,     +0.70f, 1.00f } },
    { KEY_UP,               { GRP_ARROW,     +0.80f, 1.00f } },
    { KEY_RIGHT,            { GRP_ARROW,     +0.95f, 1.00f } },
    { KEY_DOWN,             { GRP_ARROW,     +0.80f, 1.00f } },

    /* ---- Number row (VK 48..57), feel 1.55 ---- */
    { KEY_1,                { GRP_ALPHA,     -0.80f, 1.55f } },
    { KEY_2,                { GRP_ALPHA,     -0.65f, 1.55f } },
    { KEY_3,                { GRP_ALPHA,     -0.50f, 1.55f } },
    { KEY_4,                { GRP_ALPHA,     -0.35f, 1.55f } },
    { KEY_5,                { GRP_ALPHA,     -0.20f, 1.55f } },
    { KEY_6,                { GRP_ALPHA,     -0.05f, 1.55f } },
    { KEY_7,                { GRP_ALPHA,     +0.10f, 1.55f } },
    { KEY_8,                { GRP_ALPHA,     +0.25f, 1.55f } },
    { KEY_9,                { GRP_ALPHA,     +0.40f, 1.55f } },
    { KEY_0,                { GRP_ALPHA,     +0.55f, 1.55f } },

    /* ---- Letters (VK 65..90) ---- */
    { KEY_A,                { GRP_ALPHA,     -0.72f, 0.40f } },
    { KEY_B,                { GRP_ALPHA,      0.00f, 1.30f } },
    { KEY_C,                { GRP_ALPHA,     -0.35f, 1.00f } },
    { KEY_D,                { GRP_ALPHA,     -0.38f, 0.40f } },
    { KEY_E,                { GRP_ALPHA,     -0.45f, 1.00f } },
    { KEY_F,                { GRP_ALPHA,     -0.20f, 0.40f } },
    { KEY_G,                { GRP_ALPHA,     -0.03f, 1.30f } },
    { KEY_H,                { GRP_ALPHA,     +0.12f, 1.30f } },
    { KEY_I,                { GRP_ALPHA,     +0.30f, 1.00f } },
    { KEY_J,                { GRP_ALPHA,     +0.28f, 0.40f } },
    { KEY_K,                { GRP_ALPHA,     +0.45f, 0.40f } },
    { KEY_L,                { GRP_ALPHA,     +0.60f, 0.40f } },
    { KEY_M,                { GRP_ALPHA,     +0.32f, 1.00f } },
    { KEY_N,                { GRP_ALPHA,     +0.15f, 1.30f } },
    { KEY_O,                { GRP_ALPHA,     +0.45f, 1.00f } },
    { KEY_P,                { GRP_ALPHA,     +0.60f, 1.00f } },
    { KEY_Q,                { GRP_ALPHA,     -0.75f, 1.00f } },
    { KEY_R,                { GRP_ALPHA,     -0.30f, 1.00f } },
    { KEY_S,                { GRP_ALPHA,     -0.55f, 0.40f } },
    { KEY_T,                { GRP_ALPHA,     -0.15f, 1.30f } },
    { KEY_U,                { GRP_ALPHA,     +0.15f, 1.00f } },
    { KEY_V,                { GRP_ALPHA,     -0.18f, 1.00f } },
    { KEY_W,                { GRP_ALPHA,     -0.60f, 1.00f } },
    { KEY_X,                { GRP_ALPHA,     -0.52f, 1.00f } },
    { KEY_Y,                { GRP_ALPHA,      0.00f, 1.30f } },
    { KEY_Z,                { GRP_ALPHA,     -0.70f, 1.00f } },

    /* ---- Win / menu (VK 91/92/93) ---- */
    { KEY_LEFTMETA,         { GRP_MODIFIER,  -0.65f, 1.00f } },
    { KEY_RIGHTMETA,        { GRP_MODIFIER,  +0.65f, 1.00f } },
    { KEY_COMPOSE,          { GRP_MODIFIER,  +0.75f, 1.00f } },

    /* ---- F row (VK 112..123) ---- */
    { KEY_F1,               { GRP_MODIFIER,  -0.80f, 1.00f } },
    { KEY_F2,               { GRP_MODIFIER,  -0.65f, 1.00f } },
    { KEY_F3,               { GRP_MODIFIER,  -0.50f, 1.00f } },
    { KEY_F4,               { GRP_MODIFIER,  -0.35f, 1.00f } },
    { KEY_F5,               { GRP_MODIFIER,  -0.15f, 1.00f } },
    { KEY_F6,               { GRP_MODIFIER,   0.00f, 1.00f } },
    { KEY_F7,               { GRP_MODIFIER,  +0.15f, 1.00f } },
    { KEY_F8,               { GRP_MODIFIER,  +0.35f, 1.00f } },
    { KEY_F9,               { GRP_MODIFIER,  +0.50f, 1.00f } },
    { KEY_F10,              { GRP_MODIFIER,  +0.65f, 1.00f } },
    { KEY_F11,              { GRP_MODIFIER,  +0.80f, 1.00f } },
    { KEY_F12,              { GRP_MODIFIER,  +0.95f, 1.00f } },

    /* ---- Shifts / ctrl / alt (VK 160..165) ---- */
    { KEY_LEFTSHIFT,        { GRP_MODIFIER,  -0.95f, 1.00f } },
    { KEY_RIGHTSHIFT,       { GRP_MODIFIER,  +0.95f, 1.00f } },
    { KEY_LEFTCTRL,         { GRP_MODIFIER,  -0.80f, 1.00f } },
    { KEY_RIGHTCTRL,        { GRP_MODIFIER,  +0.95f, 1.00f } },
    { KEY_LEFTALT,          { GRP_MODIFIER,  -0.50f, 1.00f } },
    { KEY_RIGHTALT,         { GRP_MODIFIER,  +0.50f, 1.00f } },

    /* ---- Punctuation (VK 186..192, 219..222) ---- */
    { KEY_SEMICOLON,        { GRP_ALPHA,     +0.75f, 0.40f } },
    { KEY_EQUAL,            { GRP_ALPHA,     +0.80f, 1.85f } },
    { KEY_COMMA,            { GRP_ALPHA,     +0.48f, 1.00f } },
    { KEY_MINUS,            { GRP_ALPHA,     +0.70f, 1.85f } },
    { KEY_DOT,              { GRP_ALPHA,     +0.65f, 1.00f } },
    { KEY_SLASH,            { GRP_ALPHA,     +0.80f, 1.85f } },
    { KEY_GRAVE,            { GRP_ALPHA,     -0.95f, 1.85f } },
    { KEY_LEFTBRACE,        { GRP_ALPHA,     +0.75f, 1.85f } },
    { KEY_BACKSLASH,        { GRP_ALPHA,     +0.95f, 1.85f } },
    { KEY_RIGHTBRACE,       { GRP_ALPHA,     +0.85f, 1.85f } },
    { KEY_APOSTROPHE,       { GRP_ALPHA,     +0.85f, 1.85f } },

    /* ---- Linux/ISO/JIS extras (no VK counterpart in Keeby's map) ---- */
    { KEY_102ND,            { GRP_ALPHA,     -0.95f, 1.30f } }, /* ISO <>| next to left shift */
    { KEY_YEN,              { GRP_ALPHA,     +0.85f, 1.85f } },
    { KEY_RO,               { GRP_ALPHA,     +0.55f, 1.00f } },
    { KEY_ZENKAKUHANKAKU,       { GRP_MODIFIER,  -0.95f, 1.00f } },
    { KEY_KATAKANAHIRAGANA, { GRP_MODIFIER,  +0.20f, 1.00f } },
    { KEY_MUHENKAN,         { GRP_MODIFIER,  -0.30f, 1.00f } },
    { KEY_HENKAN,           { GRP_MODIFIER,  +0.10f, 1.00f } },
    { KEY_KATAKANA,         { GRP_MODIFIER,  +0.25f, 1.00f } },
    { KEY_HIRAGANA,         { GRP_MODIFIER,  +0.30f, 1.00f } },

    /* ---- Navigation cluster (Keeby leaves these at alpha defaults;
            we park them on modifier with plausible pans) ---- */
    { KEY_PRINT,            { GRP_MODIFIER,  +0.15f, 1.00f } },
    { KEY_SCROLLLOCK,       { GRP_MODIFIER,  +0.35f, 1.00f } },
    { KEY_PAUSE,            { GRP_MODIFIER,  +0.50f, 1.00f } },
    { KEY_INSERT,           { GRP_MODIFIER,  +0.45f, 1.00f } },
    { KEY_HOME,             { GRP_MODIFIER,  +0.55f, 1.00f } },
    { KEY_PAGEUP,           { GRP_MODIFIER,  +0.65f, 1.00f } },
    { KEY_END,              { GRP_MODIFIER,  +0.75f, 1.00f } },
    { KEY_PAGEDOWN,         { GRP_MODIFIER,  +0.85f, 1.00f } },
    { KEY_NUMLOCK,          { GRP_MODIFIER,  +0.85f, 1.00f } },

    /* ---- Numpad (right side, alpha sounds) ---- */
    { KEY_KP1,              { GRP_ALPHA,     +0.65f, 1.55f } },
    { KEY_KP2,              { GRP_ALPHA,     +0.72f, 1.55f } },
    { KEY_KP3,              { GRP_ALPHA,     +0.80f, 1.55f } },
    { KEY_KP4,              { GRP_ALPHA,     +0.60f, 1.55f } },
    { KEY_KP5,              { GRP_ALPHA,     +0.68f, 1.55f } },
    { KEY_KP6,              { GRP_ALPHA,     +0.76f, 1.55f } },
    { KEY_KP7,              { GRP_ALPHA,     +0.55f, 1.55f } },
    { KEY_KP8,              { GRP_ALPHA,     +0.63f, 1.55f } },
    { KEY_KP9,              { GRP_ALPHA,     +0.71f, 1.55f } },
    { KEY_KP0,              { GRP_ALPHA,     +0.64f, 1.55f } },
    { KEY_KPDOT,            { GRP_ALPHA,     +0.80f, 1.85f } },
    { KEY_KPPLUS,           { GRP_ALPHA,     +0.82f, 1.85f } },
    { KEY_KPMINUS,          { GRP_ALPHA,     +0.60f, 1.85f } },
    { KEY_KPASTERISK,       { GRP_ALPHA,     +0.66f, 1.85f } },
    { KEY_KPSLASH,          { GRP_ALPHA,     +0.58f, 1.85f } },
    { KEY_KPEQUAL,          { GRP_ALPHA,     +0.78f, 1.85f } },
    { KEY_KPCOMMA,          { GRP_ALPHA,     +0.70f, 1.00f } },
};

const KeyPos *keymap_lookup(unsigned short code)
{
    /* Linear scan over ~120 entries: called at most a few hundred times per
     * second — negligible. Could be a lookup table if ever needed. */
    for (size_t i = 0; i < sizeof(g_map) / sizeof(g_map[0]); i++) {
        if (g_map[i].code == code)
            return &g_map[i].pos;
    }
    return NULL;
}

const char *keymap_group_name(KeyGroup g)
{
    static const char *names[GRP_COUNT] = {
        "alpha", "space", "enter", "backspace", "modifier", "tab", "arrow", "mouse",
    };
    return names[g];
}
