#!/bin/sh

set -xe

QCOW2_IMAGE="$1"
SSH_PORT="$2"

# TODO: check if the user didn't provide enough arguments

qemu-system-aarch64 \
    -cpu cortex-a53 -smp cores=2 \
    -device virtio-gpu-pci \
    -display sdl \
    -serial mon:stdio \
    -M virt -m 1024 \
    -bios /usr/share/edk2/aarch64/QEMU_CODE.fd \
    -drive format=qcow2,file="$QCOW2_IMAGE" \
    -device e1000,netdev=net0 \
    -netdev user,id=net0,hostfwd=tcp::"$SSH_PORT"-:22 \
    -rtc base=utc,clock=host
