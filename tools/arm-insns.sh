#!/bin/sh
# ARM instruction counts of render_bench under qemu-arm, per iteration.
#
#   tools/arm-insns.sh                 build the current tree, count it
#   tools/arm-insns.sh --save before   ... and keep the binary as target/tools/before
#   tools/arm-insns.sh before          count a kept binary instead of building
#   tools/arm-insns.sh before current  compare two (current = build the tree)
#
# Env: N (iterations, default 50), SCENES (default all), PHASES (raster frame).
#
# QEMU is not cycle-accurate: these are executed guest instructions, exact and
# repeatable, so good for catching regressions and comparing variants, but
# they ignore caches, memory bandwidth and instruction latency. Confirm
# important wins on hardware. qemu counts every thread, so scenes that load
# images vary a little; a larger N dilutes that.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
out=$root/target/tools
target=armv7-unknown-linux-musleabihf
N=${N:-50}
SCENES=${SCENES:-todo todo_scroll calculator showcase vehicle music_player animation}
PHASES=${PHASES:-raster frame}
mkdir -p "$out"

plugin=$out/libinsn.so
if [ ! -f "$plugin" ] || [ "$root/tools/qemu-insn/insn.c" -nt "$plugin" ]; then
    cc -shared -fPIC -O2 $(pkg-config --cflags glib-2.0) -o "$plugin" "$root/tools/qemu-insn/insn.c"
fi

build() {
    # Static musl so qemu-arm needs no sysroot; NEON flags as on the boards.
    (cd "$root" && RUSTFLAGS="-C target-feature=+neon,+vfp4,+d32" nice cargo zigbuild -j4 --release \
        --target $target --no-default-features --example render_bench >&2)
    echo "$root/target/$target/release/examples/render_bench"
}

save=
if [ "${1:-}" = "--save" ]; then
    save=$2
    shift 2
fi
[ $# -eq 0 ] && set -- current

bins=
for name in "$@"; do
    if [ "$name" = current ]; then
        bin=$(build)
        if [ -n "$save" ]; then cp "$bin" "$out/$save"; fi
    else
        bin=$out/$name
        [ -f "$bin" ] || { echo "no saved binary $bin" >&2; exit 1; }
    fi
    bins="$bins $name=$bin"
done

count() { # binary scene phase iterations
    RENDER_BENCH_SCENES=$2 RENDER_BENCH_PHASE=$3 RENDER_BENCH_ITERS=$4 \
        qemu-arm -plugin "$plugin" "$1" 2>&1 >/dev/null | awk '/guest_insns/ { print $2 }'
}

printf '%-14s %-7s' scene phase
for b in $bins; do printf ' %14s' "${b%%=*}"; done
echo
for scene in $SCENES; do
    for phase in $PHASES; do
        printf '%-14s %-7s' "$scene" "$phase"
        for b in $bins; do
            bin=${b#*=}
            c0=$(count "$bin" "$scene" "$phase" 0)
            cn=$(count "$bin" "$scene" "$phase" "$N")
            printf ' %12.2f M' "$(echo "($cn - $c0) / $N / 1000000" | bc -l)"
        done
        echo
    done
done
