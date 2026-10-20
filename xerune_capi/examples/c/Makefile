# Builds the shop sample against xerune's C API.
#
# Build the Rust library first, for the same target, from the repository
# root:
#   cargo build --release -p xerune_capi                                    # Linux
#   cargo zigbuild --release -p xerune_capi --target x86_64-pc-windows-gnu  # Windows
# then here:
#   make run        # Linux: builds ./shop and runs it
#   make shop.exe   # Windows, cross-compiled with zig cc (or set WINCC to MinGW's gcc)
# or point XERUNE_LIB at a libxerune_capi.a built elsewhere.

ROOT := ../../..
CFLAGS ?= -O2 -Wall -Wextra
CPPFLAGS += -I../../include

XERUNE_LIB ?= $(ROOT)/target/release/libxerune_capi.a
# What the Rust standard library needs from the system
# (`cargo rustc -p xerune_capi -- --print native-static-libs`).
LIBS := -lgcc_s -lutil -lrt -lpthread -lm -ldl

XERUNE_WIN_LIB ?= $(ROOT)/target/x86_64-pc-windows-gnu/release/libxerune_capi.a
WINCC ?= zig cc -target x86_64-windows-gnu
# Rust's windows-gnu standard library unwinds with libgcc's _Unwind_*
# functions: MinGW's gcc has them, zig cc takes them from LLVM's libunwind.
WINLIBS := -lkernel32 -ladvapi32 -lntdll -luserenv -lws2_32 -lbcrypt -ldbghelp $(if $(findstring zig,$(WINCC)),-lunwind)

shop: shop.c ../../include/xerune.h $(XERUNE_LIB)
	$(CC) $(CPPFLAGS) $(CFLAGS) -o $@ shop.c $(XERUNE_LIB) $(LIBS)

shop.exe: shop.c ../../include/xerune.h $(XERUNE_WIN_LIB)
	$(WINCC) $(CPPFLAGS) $(CFLAGS) -o $@ shop.c $(XERUNE_WIN_LIB) $(WINLIBS)

# The template's path in shop.c is relative to this directory.
run: shop
	./shop

clean:
	rm -f shop shop.exe shop.pdb shop.lib shop.ppm

.PHONY: run clean
