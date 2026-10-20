# An in-game PC showing xerune's UI, drawn with raylib: a web shop on a
# monitor in a low-poly room. Needs raylib 5 or newer.
#
# Build the Rust library first, from the repository root:
#   cargo build --release -p xerune_capi
# then here:
#   make run      walk around: WASD, mouse, C sits/stands, click the screen
#   make video    pc.mp4, a scripted visit (needs ffmpeg)
# The README's clip: ./pc --record ../../../docs/img/pc.webp

ROOT := ../../..
CFLAGS ?= -O2 -Wall -Wextra
CPPFLAGS += -I../../include

XERUNE_LIB ?= $(ROOT)/target/release/libxerune_capi.a
# raylib, and what Rust's standard library needs from the system.
LIBS := -lraylib -lgcc_s -lutil -lrt -lpthread -lm -ldl

pc: pc.c ../../include/xerune.h $(XERUNE_LIB)
	$(CC) $(CPPFLAGS) $(CFLAGS) -o $@ pc.c $(XERUNE_LIB) $(LIBS)

# The template's path in pc.c is relative to this directory.
run: pc
	./pc

video: pc
	./pc --record pc.mp4

clean:
	rm -f pc pc.mp4 *.ppm

.PHONY: run video clean
