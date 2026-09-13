CC      ?= cc
CFLAGS  ?= -O2 -std=gnu17 -Wall -Wextra -Wno-unused-parameter -Wno-missing-field-initializers -Ivendor
LDFLAGS ?= -lm -lpthread -ldl -lrt

SRC := src/main.c src/audio.c src/sound.c src/input.c src/keymap.c src/config.c src/catalog.c src/http.c src/miniaudio_impl.c
BIN := bin/keebyd

all: $(BIN)

$(BIN): $(SRC) $(wildcard src/*.h) vendor/miniaudio.h
	@mkdir -p bin
	$(CC) $(CFLAGS) -Isrc -o $@ $(SRC) $(LDFLAGS)

clean:
	rm -rf bin

.PHONY: all clean
