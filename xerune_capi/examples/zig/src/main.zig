//! A shop drawn by xerune, driven from Zig through the C API: render,
//! click Buy, handle the message by updating the model, render again, and
//! write the frame to a PPM file. A game does the same each frame, with
//! real input, and uploads the damaged area to a texture.
//!
//!   zig build run -- [template] [out.ppm]

const std = @import("std");
const xr = @import("xerune");

const width = 640;
const height = 400;

/// The model the template shows, as JSON: the template reads these fields.
const Item = struct {
    id: u32,
    name: []const u8,
    price: i64,
    owned: bool = false,
};

const Model = struct {
    gold: i64,
    items: []Item,
    /// null until something is bought (`{% if let Some(..) %}`).
    last_purchase: ?[]const u8 = null,
    owned_share: f64 = 0,
};

fn fail(what: []const u8) error{Xerune} {
    std.debug.print("{s}: {s}\n", .{ what, std.mem.span(xr.xr_last_error()) });
    return error.Xerune;
}

fn setModel(gpa: std.mem.Allocator, ui: *xr.XrUi, model: Model) !void {
    const json = try std.json.Stringify.valueAlloc(gpa, model, .{});
    defer gpa.free(json);
    const json_z = try gpa.dupeZ(u8, json);
    defer gpa.free(json_z);
    if (xr.xr_set_model(ui, json_z) != 0) return fail("xr_set_model");
}

/// Presses and releases the pointer at (`x`, `y`).
fn click(ui: *xr.XrUi, x: f32, y: f32) void {
    _ = xr.xr_pointer_move(ui, x, y);
    _ = xr.xr_pointer_down(ui, x, y);
    _ = xr.xr_pointer_up(ui, x, y);
}

/// Handles a message of the view: "buy:<id>".
fn update(model: *Model, message: []const u8) void {
    const prefix = "buy:";
    if (!std.mem.startsWith(u8, message, prefix)) return;
    const id = std.fmt.parseInt(u32, message[prefix.len..], 10) catch return;
    var owned: f64 = 0;
    for (model.items) |*item| {
        if (item.id == id and !item.owned and item.price <= model.gold) {
            model.gold -= item.price;
            item.owned = true;
            model.last_purchase = item.name;
        }
        if (item.owned) owned += 1;
    }
    model.owned_share = owned / @as(f64, @floatFromInt(model.items.len));
}

fn writePpm(io: std.Io, gpa: std.mem.Allocator, path: []const u8, pixels: []const u32) !void {
    const header = std.fmt.comptimePrint("P6\n{d} {d}\n255\n", .{ width, height });
    const data = try gpa.alloc(u8, header.len + pixels.len * 3);
    defer gpa.free(data);
    @memcpy(data[0..header.len], header);
    for (pixels, 0..) |pixel, i| {
        // XR_PIXELS_XRGB8888: 0xXXRRGGBB.
        const rgb = data[header.len + i * 3 ..][0..3];
        rgb.* = .{ @truncate(pixel >> 16), @truncate(pixel >> 8), @truncate(pixel) };
    }
    try std.Io.Dir.cwd().writeFile(io, .{ .sub_path = path, .data = data });
}

pub fn main(init: std.process.Init) !void {
    const gpa = init.gpa;
    const args = try init.minimal.args.toSlice(init.arena.allocator());
    const template: [:0]const u8 = if (args.len > 1) args[1] else "ui/shop.html";
    const out: []const u8 = if (args.len > 2) args[2] else "shop.ppm";

    std.debug.print("xerune {s}\n", .{std.mem.span(xr.xr_version())});
    const ui = xr.xr_new(null, 0) orelse return fail("xr_new");
    defer xr.xr_free(ui);
    if (xr.xr_load_template(ui, template) != 0) return fail("xr_load_template");

    var items = [_]Item{
        .{ .id = 1, .name = "Iron sword", .price = 40 },
        .{ .id = 2, .name = "Health potion", .price = 15 },
        .{ .id = 3, .name = "Dragon shield", .price = 400 },
    };
    var model = Model{ .gold = 100, .items = &items };
    try setModel(gpa, ui, model);
    xr.xr_set_size(ui, width, height);

    const pixels = try gpa.alloc(u32, width * height);
    defer gpa.free(pixels);
    var damage: xr.XrRect = undefined;
    var now_ms: u64 = 0;
    _ = xr.xr_tick(ui, now_ms);
    if (!xr.xr_render(ui, pixels.ptr, width, xr.XR_PIXELS_XRGB8888, &damage)) return fail("xr_render");
    std.debug.print("first frame: {d}x{d}\n", .{ damage.width, damage.height });

    // Buy the potion: its Buy button is in the second row.
    click(ui, 573, 146);
    while (xr.xr_next_message(ui)) |message| {
        const text = std.mem.span(message);
        std.debug.print("message: {s}\n", .{text});
        update(&model, text);
    }
    try setModel(gpa, ui, model);

    // A few frames, as a game loop would run them.
    for (0..20) |_| {
        now_ms += 16;
        _ = xr.xr_tick(ui, now_ms);
        if (xr.xr_render(ui, pixels.ptr, width, xr.XR_PIXELS_XRGB8888, &damage)) {
            std.debug.print("frame at {d} ms: redrew {d}x{d} at ({d}, {d})\n", .{ now_ms, damage.width, damage.height, damage.x, damage.y });
        }
    }
    std.debug.print("gold left: {d}\n", .{model.gold});

    try writePpm(init.io, gpa, out, pixels);
    std.debug.print("wrote {s}\n", .{out});
}
