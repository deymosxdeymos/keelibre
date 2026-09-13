/*
 * miniaudio_impl.c — the single translation unit that compiles miniaudio.
 * Every other file includes <miniaudio.h> for declarations only.
 */
#define MA_NO_ENCODING
#define MA_NO_GENERATION
#define MA_NO_RESOURCE_MANAGER
#define MINIAUDIO_IMPLEMENTATION
#include <miniaudio.h>
