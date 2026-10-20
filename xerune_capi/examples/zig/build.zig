//! Builds the shop sample against xerune's C API.
//!
//! Build the Rust library first, for the same target, from the repository
//! root:
//!   cargo build --release -p xerune_capi                                    # Linux
//!   cargo zigbuild --release -p xerune_capi --target x86_64-pc-windows-gnu  # Windows
//! then here:
//!   zig build run                          # Linux
//!   zig build -Dtarget=x86_64-windows-gnu  # Windows (zig-out/bin/shop.exe)
//! or point -Dxerune-lib at a libxerune_capi.a built elsewhere.

const std = @import("std");

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});
    const os = target.result.os.tag;

    const default_lib = switch (os) {
        .windows => "../../../target/x86_64-pc-windows-gnu/release/libxerune_capi.a",
        else => "../../../target/release/libxerune_capi.a",
    };
    const lib = b.option([]const u8, "xerune-lib", "The libxerune_capi.a to link") orelse default_lib;

    // The header as a Zig module: `@import("xerune")`.
    const header = b.addTranslateC(.{
        .root_source_file = b.path("../../include/xerune.h"),
        .target = target,
        .optimize = optimize,
    });

    const exe = b.addExecutable(.{
        .name = "shop",
        .root_module = b.createModule(.{
            .root_source_file = b.path("src/main.zig"),
            .target = target,
            .optimize = optimize,
            .link_libc = true,
            // Rust's windows-gnu standard library unwinds with libgcc's
            // _Unwind_* functions; Zig provides them with LLVM's
            // libunwind, which comes with libc++.
            .link_libcpp = os == .windows,
            .imports = &.{.{ .name = "xerune", .module = header.createModule() }},
        }),
    });
    exe.root_module.addObjectFile(b.path(lib));
    // What the Rust standard library needs from the system
    // (`cargo rustc -- --print native-static-libs`).
    const system_libs: []const []const u8 = switch (os) {
        .windows => &.{ "kernel32", "advapi32", "ntdll", "userenv", "ws2_32", "bcrypt", "dbghelp" },
        else => &.{ "gcc_s", "util", "rt", "pthread", "m", "dl" },
    };
    for (system_libs) |name| exe.root_module.linkSystemLibrary(name, .{});
    b.installArtifact(exe);

    const run = b.addRunArtifact(exe);
    // The template's path in main.zig is relative to this directory.
    run.setCwd(b.path("."));
    if (b.args) |args| run.addArgs(args);
    b.step("run", "Run the shop sample").dependOn(&run.step);
}
