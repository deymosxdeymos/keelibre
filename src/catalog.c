#include <string.h>
#include <stddef.h>
#include "catalog.h"

const SwitchMeta CATALOG[] = {
    { "aflion-carrot", "Carrot Orange", "Aflion", "Tactile · 37g", "#ED802E", "", 0.96f },
    { "akko-piano-pro", "Piano Pro", "Akko", "Linear · 45g", "#9E73C7", "Alice (@aliceinnny)", 3.0f },
    { "akko-cs-jelly-black", "CS Jelly Black", "Akko", "Linear · 50g", "#292930", "", 3.5f },
    { "akko-v3-pro-cream-yellow", "V3 Cream Yellow Pro", "Akko", "Linear · 50g", "#F5DB80", "", 3.5f },
    { "akko-clicky-pink", "Clicky Pink", "Akko", "Clicky · 50g", "#ED739E", "", 0.9f },
    { "lofree-flow-2-surfer", "Surfer", "Lofree", "Low-profile linear · 40g", "#FFFFFF", "", 3.5f },
    { "lofree-flow-2-void", "Void", "Lofree", "Low-profile silent · 40g", "#CCCCD1", "", 3.5f },
    { "lofree-flow-2-pulse", "Pulse", "Lofree", "Low-profile tactile · 40g", "#42424A", "", 3.5f },
    { "alps-skcm-blue", "SKCM Blue", "Alps", "Clicky · 70g", "#4D80CC", "", 0.85f },
    { "drop-holy-panda", "Holy Panda", "Drop", "Tactile · 67g", "#D98C33", "", 0.9f },
    { "durock-alpaca", "Alpaca", "Durock", "Linear · 62g", "#E6CCB3", "", 1.0f },
    { "gateron-ink-black", "Ink Black", "Gateron", "Linear · 60g", "#333340", "", 1.0f },
    { "gateron-ink-red", "Ink Red", "Gateron", "Linear · 45g", "#C74747", "", 1.0f },
    { "gateron-turquoise-tealios", "Turquoise Tealios", "Gateron", "Linear · 63.5g", "#40BFB3", "", 1.0f },
    { "ibm-buckling-spring", "Buckling Spring", "IBM", "Clicky · 65g", "#B3B3A6", "", 0.7f },
    { "iqunix-mq80", "MQ80", "IQUNIX", "Low-profile linear · 40g", "#4DA6D9", "Alex (@aliszu)", 0.75f },
    { "kailh-box-navy", "Box Navy", "Kailh", "Clicky · 75g", "#26408C", "", 0.8f },
    { "keychron-k2-max-red", "Gateron Red", "Keychron", "Linear · 45g", "#C74747", "", 0.55f },
    { "keychron-k2-max-brown", "Gateron Brown", "Keychron", "Tactile · 55g", "#8C7047", "Himanshu (@himanhacks)", 0.85f },
    { "lizard", "Lizard", "Quirky", "Gecko · lol", "#73BF4D", "", 1.0f },
    { "novelkeys-cream", "Cream", "NovelKeys", "Linear · 55g", "#F2E6CC", "", 1.0f },
    { "topre-classic", "Classic", "Topre", "Tactile · 45g", "#998CA6", "", 1.0f },
};
const int CATALOG_LEN = (int)(sizeof(CATALOG) / sizeof(CATALOG[0]));

const SwitchMeta *catalog_find(const char *dir)
{
    for (int i = 0; i < CATALOG_LEN; i++)
        if (strcmp(CATALOG[i].dir, dir) == 0) return &CATALOG[i];
    return NULL;
}
