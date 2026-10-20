/* An in-game PC: a low-poly room with a desk and a monitor, and on the
 * monitor a web shop drawn by xerune. Aim at the screen and click.
 *
 *   ./pc                       walk around: WASD, mouse, C sits/stands, click
 *   ./pc --record out.mp4      a scripted visit, rendered at 60 fps into
 *                              ffmpeg (.webp: an animated webp, as in the README)
 *   ./pc --shot ui.ppm [ms [message...]]
 *                              just the screen's UI at a time, after messages
 *                              ("buy:2"), for finding coordinates
 *
 * The template is ../ui/market.html.
 */

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "raylib.h"
#include "raymath.h"
#include "rlgl.h"
#include "xerune.h"

/* The UI's size in pixels: the screen texture. */
enum { UI_W = 960, UI_H = 600 };

/* The screen in the world: 0.64 m x 0.40 m, facing +z. */
static const Vector3 SCREEN_TL = {-0.32f, 1.25f, -0.2245f};
static const Vector3 SCREEN_TR = {0.32f, 1.25f, -0.2245f};
static const Vector3 SCREEN_BL = {-0.32f, 0.85f, -0.2245f};
static const Vector3 SCREEN_BR = {0.32f, 0.85f, -0.2245f};
static const Vector3 SCREEN_C = {0.0f, 1.05f, -0.2245f};
static const Vector3 LAMP = {0.62f, 1.17f, -0.36f};

/* ------------------------------------------------------------------ */
/* The model, as JSON for xerune.                                      */

struct item {
    int id;
    const char *name, *kind, *color;
    long price;
    bool owned;
};

static struct item items[] = {
    {1, "Iron sword", "One-handed", "#9aa3b5", 40, false},
    {2, "Health potion", "Restores 50 HP", "#e0503d", 15, false},
    {3, "Dragon shield", "Heavy armor", "#3dc07a", 400, false},
    {4, "Oak bow", "Two-handed", "#c08a3d", 60, false},
    {5, "Mana flask", "Restores 40 MP", "#3d7be0", 25, false},
    {6, "Leather boots", "Light armor", "#a07850", 30, false},
};
enum { N_ITEMS = sizeof items / sizeof items[0] };
static long gold = 150;
static const char *last_purchase;

static void fail(const char *what) {
    fprintf(stderr, "%s: %s\n", what, xr_last_error());
    exit(1);
}

static void set_model(XrUi *ui) {
    char json[4096];
    size_t n = 0;
    n += snprintf(json + n, sizeof json - n, "{\"gold\": %ld, \"spent\": %s, \"items\": [", gold, last_purchase ? "true" : "false");
    for (int i = 0; i < N_ITEMS; i++)
        n += snprintf(json + n, sizeof json - n,
                      "%s{\"id\": %d, \"name\": \"%s\", \"kind\": \"%s\", \"color\": \"%s\", \"price\": %ld, \"owned\": %s}",
                      i ? ", " : "", items[i].id, items[i].name, items[i].kind, items[i].color, items[i].price,
                      items[i].owned ? "true" : "false");
    if (last_purchase)
        snprintf(json + n, sizeof json - n, "], \"last_purchase\": \"%s\"}", last_purchase);
    else
        snprintf(json + n, sizeof json - n, "], \"last_purchase\": null}");
    if (xr_set_model(ui, json) != 0)
        fail("xr_set_model");
}

/* Handles a message of the view: "buy:<id>". True if the model changed. */
static bool update(const char *m) {
    if (strncmp(m, "buy:", 4) != 0)
        return false;
    int id = atoi(m + 4);
    for (int i = 0; i < N_ITEMS; i++) {
        if (items[i].id == id && !items[i].owned && items[i].price <= gold) {
            gold -= items[i].price;
            items[i].owned = true;
            last_purchase = items[i].name;
            printf("bought %s, %ld gold left\n", items[i].name, gold);
            return true;
        }
    }
    return false;
}

static void handle_messages(XrUi *ui) {
    bool changed = false;
    for (const char *m; (m = xr_next_message(ui));)
        changed |= update(m);
    if (changed)
        set_model(ui);
}

/* ------------------------------------------------------------------ */
/* Low-poly drawing: flat-colored boxes lit per vertex.                */

static unsigned char clamp255(float v) { return (unsigned char)(v < 0 ? 0 : v > 255 ? 255 : v); }

/* An evening room: dim cool ambient, moonlight through the window, the
 * warm desk lamp, and the screen's bluish glow. */
static Color shade(Vector3 p, Vector3 n, Color base) {
    float r = 0.24f, g = 0.25f, b = 0.33f;

    Vector3 moon = Vector3Normalize((Vector3){0.35f, 0.45f, 1.0f});
    float m = fmaxf(0, Vector3DotProduct(n, moon)) * 0.22f;
    r += m * 0.7f, g += m * 0.8f, b += m * 1.0f;

    Vector3 d = Vector3Subtract(LAMP, p);
    float l = fmaxf(0, Vector3DotProduct(n, Vector3Normalize(d))) * 1.1f / (1.0f + Vector3LengthSqr(d) * 2.5f);
    r += l * 1.0f, g += l * 0.75f, b += l * 0.45f;

    Vector3 s = Vector3Subtract(SCREEN_C, p);
    if (p.z > SCREEN_C.z) {
        Vector3 sn = Vector3Normalize(s);
        float facing = fmaxf(0, -sn.z); /* how much p is in front of the screen */
        float sl = fmaxf(0, Vector3DotProduct(n, sn)) * facing * 0.9f / (1.0f + Vector3LengthSqr(s) * 3.0f);
        r += sl * 0.55f, g += sl * 0.7f, b += sl * 1.0f;
    }
    return (Color){clamp255(base.r * r * 1.6f), clamp255(base.g * g * 1.6f), clamp255(base.b * b * 1.6f), base.a};
}

static void vertex(Vector3 p, Vector3 n, Color base, bool lit) {
    Color c = lit ? shade(p, n, base) : base;
    rlColor4ub(c.r, c.g, c.b, c.a);
    rlVertex3f(p.x, p.y, p.z);
}

/* A face from `o` along `u` and `v`, split into tiles so that the lights
 * fall off smoothly across it. */
static void face(Vector3 o, Vector3 u, Vector3 v, Vector3 n, Color base, bool lit) {
    int nu = (int)ceilf(Vector3Length(u) / 0.2f), nv = (int)ceilf(Vector3Length(v) / 0.2f);
    if (!lit)
        nu = nv = 1;
    rlCheckRenderBatchLimit(nu * nv * 4);
    rlBegin(RL_QUADS);
    for (int i = 0; i < nu; i++) {
        for (int j = 0; j < nv; j++) {
            Vector3 a = Vector3Add(o, Vector3Add(Vector3Scale(u, (float)i / nu), Vector3Scale(v, (float)j / nv)));
            Vector3 du = Vector3Scale(u, 1.0f / nu), dv = Vector3Scale(v, 1.0f / nv);
            vertex(a, n, base, lit);
            vertex(Vector3Add(a, du), n, base, lit);
            vertex(Vector3Add(a, Vector3Add(du, dv)), n, base, lit);
            vertex(Vector3Add(a, dv), n, base, lit);
        }
    }
    rlEnd();
}

static void box_lit(Vector3 c, Vector3 s, Color col, bool lit) {
    float x = s.x / 2, y = s.y / 2, z = s.z / 2;
    face((Vector3){c.x + x, c.y - y, c.z - z}, (Vector3){0, 0, s.z}, (Vector3){0, s.y, 0}, (Vector3){1, 0, 0}, col, lit);
    face((Vector3){c.x - x, c.y - y, c.z - z}, (Vector3){0, 0, s.z}, (Vector3){0, s.y, 0}, (Vector3){-1, 0, 0}, col, lit);
    face((Vector3){c.x - x, c.y + y, c.z - z}, (Vector3){s.x, 0, 0}, (Vector3){0, 0, s.z}, (Vector3){0, 1, 0}, col, lit);
    face((Vector3){c.x - x, c.y - y, c.z - z}, (Vector3){s.x, 0, 0}, (Vector3){0, 0, s.z}, (Vector3){0, -1, 0}, col, lit);
    face((Vector3){c.x - x, c.y - y, c.z + z}, (Vector3){s.x, 0, 0}, (Vector3){0, s.y, 0}, (Vector3){0, 0, 1}, col, lit);
    face((Vector3){c.x - x, c.y - y, c.z - z}, (Vector3){s.x, 0, 0}, (Vector3){0, s.y, 0}, (Vector3){0, 0, -1}, col, lit);
}

static void box(Vector3 c, Vector3 s, Color col) { box_lit(c, s, col, true); }

/* A low-poly leaf blob: an octahedron, each triangle flat. */
static void blob(Vector3 c, Vector3 r, Color col) {
    Vector3 p[6] = {
        {c.x + r.x, c.y, c.z}, {c.x - r.x, c.y, c.z}, {c.x, c.y + r.y, c.z},
        {c.x, c.y - r.y, c.z}, {c.x, c.y, c.z + r.z}, {c.x, c.y, c.z - r.z},
    };
    int tris[8][3] = {{0, 2, 4}, {4, 2, 1}, {1, 2, 5}, {5, 2, 0}, {0, 4, 3}, {4, 1, 3}, {1, 5, 3}, {5, 0, 3}};
    rlCheckRenderBatchLimit(24);
    rlBegin(RL_TRIANGLES);
    for (int i = 0; i < 8; i++) {
        Vector3 a = p[tris[i][0]], b = p[tris[i][1]], d = p[tris[i][2]];
        Vector3 n = Vector3Normalize(Vector3CrossProduct(Vector3Subtract(b, a), Vector3Subtract(d, a)));
        Vector3 centre = Vector3Scale(Vector3Add(a, Vector3Add(b, d)), 1.0f / 3);
        if (Vector3DotProduct(n, Vector3Subtract(centre, c)) < 0)
            n = Vector3Negate(n);
        Color s = shade(centre, n, col); /* flat: one color per triangle */
        rlColor4ub(s.r, s.g, s.b, 255);
        rlVertex3f(a.x, a.y, a.z);
        rlVertex3f(b.x, b.y, b.z);
        rlVertex3f(d.x, d.y, d.z);
    }
    rlEnd();
}

static void draw_room(Texture2D screen) {
    rlDisableBackfaceCulling();
    Color wall = {92, 96, 116, 255}, wood = {128, 90, 62, 255}, dark = {34, 35, 40, 255};

    /* Floor, ceiling, walls. */
    face((Vector3){-3.5f, 0, -0.7f}, (Vector3){7, 0, 0}, (Vector3){0, 0, 5.2f}, (Vector3){0, 1, 0}, (Color){110, 82, 60, 255}, true);
    face((Vector3){-3.5f, 2.8f, -0.7f}, (Vector3){7, 0, 0}, (Vector3){0, 0, 5.2f}, (Vector3){0, -1, 0}, (Color){70, 70, 80, 255}, true);
    face((Vector3){-3.5f, 0, -0.7f}, (Vector3){7, 0, 0}, (Vector3){0, 2.8f, 0}, (Vector3){0, 0, 1}, wall, true);
    face((Vector3){-3.5f, 0, 4.5f}, (Vector3){7, 0, 0}, (Vector3){0, 2.8f, 0}, (Vector3){0, 0, -1}, wall, true);
    face((Vector3){-3.5f, 0, -0.7f}, (Vector3){0, 0, 5.2f}, (Vector3){0, 2.8f, 0}, (Vector3){1, 0, 0}, wall, true);
    face((Vector3){3.5f, 0, -0.7f}, (Vector3){0, 0, 5.2f}, (Vector3){0, 2.8f, 0}, (Vector3){-1, 0, 0}, wall, true);
    box((Vector3){0, 0.05f, -0.69f}, (Vector3){7, 0.1f, 0.02f}, (Color){60, 60, 70, 255}); /* skirting */

    /* Window with the night sky, and its frame. */
    face((Vector3){-2.6f, 1.0f, -0.695f}, (Vector3){1.4f, 0, 0}, (Vector3){0, 1.2f, 0}, (Vector3){0, 0, 1}, (Color){22, 34, 70, 255}, false);
    rlCheckRenderBatchLimit(24); /* the moon, an octagon */
    rlBegin(RL_TRIANGLES);
    rlColor4ub(232, 232, 214, 255);
    for (int i = 0; i < 8; i++) {
        float a0 = i * PI / 4, a1 = (i + 1) * PI / 4;
        rlVertex3f(-2.3f, 1.9f, -0.69f);
        rlVertex3f(-2.3f + 0.07f * cosf(a0), 1.9f + 0.07f * sinf(a0), -0.69f);
        rlVertex3f(-2.3f + 0.07f * cosf(a1), 1.9f + 0.07f * sinf(a1), -0.69f);
    }
    rlEnd();
    Color frame = {200, 200, 205, 255};
    box((Vector3){-1.9f, 1.0f, -0.67f}, (Vector3){1.5f, 0.06f, 0.06f}, frame);
    box((Vector3){-1.9f, 2.2f, -0.67f}, (Vector3){1.5f, 0.06f, 0.06f}, frame);
    box((Vector3){-2.6f, 1.6f, -0.67f}, (Vector3){0.06f, 1.26f, 0.06f}, frame);
    box((Vector3){-1.2f, 1.6f, -0.67f}, (Vector3){0.06f, 1.26f, 0.06f}, frame);
    box((Vector3){-1.9f, 1.6f, -0.67f}, (Vector3){0.04f, 1.2f, 0.04f}, frame);
    box((Vector3){-1.9f, 0.97f, -0.6f}, (Vector3){1.6f, 0.04f, 0.2f}, frame); /* sill */

    /* Rug. */
    box((Vector3){0, 0.005f, 1.4f}, (Vector3){2.2f, 0.01f, 1.6f}, (Color){132, 48, 54, 255});

    /* Desk. */
    box((Vector3){0, 0.74f, -0.3f}, (Vector3){1.6f, 0.04f, 0.75f}, wood);
    for (int i = 0; i < 4; i++)
        box((Vector3){i % 2 ? 0.75f : -0.75f, 0.36f, i / 2 ? 0.03f : -0.62f}, (Vector3){0.05f, 0.72f, 0.05f}, wood);

    /* Monitor: body, neck, foot; the screen comes last. */
    box((Vector3){0, 1.05f, -0.25f}, (Vector3){0.70f, 0.46f, 0.05f}, dark);
    box((Vector3){0, 0.84f, -0.29f}, (Vector3){0.07f, 0.16f, 0.04f}, dark);
    box((Vector3){0, 0.77f, -0.28f}, (Vector3){0.26f, 0.02f, 0.18f}, dark);

    /* Keyboard, mouse, mug. */
    box((Vector3){0, 0.77f, -0.02f}, (Vector3){0.44f, 0.02f, 0.14f}, (Color){52, 54, 62, 255});
    for (int row = 0; row < 4; row++)
        for (int col = 0; col < 14; col++)
            box((Vector3){-0.182f + col * 0.028f, 0.783f, -0.062f + row * 0.028f}, (Vector3){0.022f, 0.008f, 0.022f}, (Color){96, 98, 108, 255});
    box((Vector3){0.33f, 0.775f, -0.01f}, (Vector3){0.06f, 0.03f, 0.1f}, (Color){44, 46, 52, 255});
    box((Vector3){-0.48f, 0.81f, -0.12f}, (Vector3){0.08f, 0.1f, 0.08f}, (Color){170, 60, 50, 255});

    /* Desk lamp; its bulb is a light, so unlit. */
    Color brass = {190, 160, 90, 255};
    box((Vector3){0.62f, 0.775f, -0.48f}, (Vector3){0.14f, 0.03f, 0.14f}, brass);
    box((Vector3){0.62f, 0.98f, -0.48f}, (Vector3){0.025f, 0.4f, 0.025f}, brass);
    box((Vector3){0.62f, 1.17f, -0.42f}, (Vector3){0.025f, 0.025f, 0.12f}, brass);
    box_lit((Vector3){0.62f, 1.22f, -0.36f}, (Vector3){0.17f, 0.08f, 0.17f}, (Color){214, 170, 96, 255}, false); /* the shade glows */
    box_lit((Vector3){0.62f, 1.175f, -0.36f}, (Vector3){0.08f, 0.012f, 0.08f}, (Color){255, 232, 180, 255}, false);

    /* Chair. */
    Color chair = {64, 68, 88, 255};
    box((Vector3){0, 0.46f, 0.6f}, (Vector3){0.48f, 0.06f, 0.46f}, chair);
    box((Vector3){0, 0.8f, 0.82f}, (Vector3){0.46f, 0.62f, 0.05f}, chair);
    for (int i = 0; i < 4; i++)
        box((Vector3){i % 2 ? 0.2f : -0.2f, 0.215f, i / 2 ? 0.8f : 0.4f}, (Vector3){0.04f, 0.43f, 0.04f}, chair);

    /* Bookshelf on the right wall: boards, and books of all heights. */
    Color shelf = {104, 70, 46, 255};
    box((Vector3){3.48f, 1.0f, 1.4f}, (Vector3){0.04f, 2.0f, 1.4f}, shelf);
    box((Vector3){3.3f, 1.0f, 0.7f}, (Vector3){0.36f, 2.0f, 0.04f}, shelf);
    box((Vector3){3.3f, 1.0f, 2.1f}, (Vector3){0.36f, 2.0f, 0.04f}, shelf);
    Color book_colors[] = {{150, 50, 50, 255}, {50, 90, 150, 255}, {200, 170, 80, 255}, {60, 120, 80, 255}, {120, 70, 130, 255}, {180, 180, 170, 255}};
    for (int s = 0; s < 4; s++) {
        float y = 0.05f + s * 0.5f;
        box((Vector3){3.3f, y, 1.4f}, (Vector3){0.36f, 0.03f, 1.36f}, shelf);
        if (s == 3)
            break;
        float z = 0.76f;
        for (int b = 0; z < 2.0f; b++) {
            float w = 0.03f + ((b * 7 + s * 3) % 4) * 0.012f, h = 0.26f + ((b * 5 + s) % 5) * 0.035f;
            if ((b + s) % 9 == 4) { /* a gap now and then */
                z += 0.12f;
                continue;
            }
            box((Vector3){3.32f, y + 0.015f + h / 2, z + w / 2}, (Vector3){0.24f, h, w}, book_colors[(b * 5 + s * 2) % 6]);
            z += w + 0.004f;
        }
    }

    /* A plant in the corner. */
    box((Vector3){-3.1f, 0.2f, -0.3f}, (Vector3){0.34f, 0.4f, 0.34f}, (Color){170, 100, 70, 255});
    Color leaf = {70, 140, 70, 255};
    blob((Vector3){-3.1f, 0.62f, -0.3f}, (Vector3){0.28f, 0.22f, 0.28f}, leaf);
    blob((Vector3){-3.05f, 0.9f, -0.25f}, (Vector3){0.22f, 0.2f, 0.22f}, (Color){80, 155, 75, 255});
    blob((Vector3){-3.12f, 1.12f, -0.32f}, (Vector3){0.14f, 0.15f, 0.14f}, (Color){95, 170, 85, 255});

    /* The screen shows the UI, unlit. Last, because rlSetTexture(0)
     * doesn't go back to raylib's white texture: what is drawn after
     * would be tinted by the UI. */
    rlCheckRenderBatchLimit(4);
    rlSetTexture(screen.id);
    rlBegin(RL_QUADS);
    rlColor4ub(255, 255, 255, 255);
    rlTexCoord2f(0, 0), rlVertex3f(SCREEN_TL.x, SCREEN_TL.y, SCREEN_TL.z);
    rlTexCoord2f(0, 1), rlVertex3f(SCREEN_BL.x, SCREEN_BL.y, SCREEN_BL.z);
    rlTexCoord2f(1, 1), rlVertex3f(SCREEN_BR.x, SCREEN_BR.y, SCREEN_BR.z);
    rlTexCoord2f(1, 0), rlVertex3f(SCREEN_TR.x, SCREEN_TR.y, SCREEN_TR.z);
    rlEnd();
    rlSetTexture(0);
}

/* ------------------------------------------------------------------ */
/* The screen as input: where the view's center hits it.               */

static Vector3 screen_point(float u, float v) {
    return Vector3Add(SCREEN_TL, Vector3Add(Vector3Scale(Vector3Subtract(SCREEN_TR, SCREEN_TL), u),
                                            Vector3Scale(Vector3Subtract(SCREEN_BL, SCREEN_TL), v)));
}

static Vector3 ui_point(float x, float y) { return screen_point(x / UI_W, y / UI_H); }

/* Whether the camera aims at the screen from close enough to use it, and
 * where, in UI pixels. */
static bool aim(Camera3D cam, Vector2 *at) {
    Ray ray = {cam.position, Vector3Normalize(Vector3Subtract(cam.target, cam.position))};
    RayCollision hit = GetRayCollisionQuad(ray, SCREEN_TL, SCREEN_TR, SCREEN_BR, SCREEN_BL);
    if (!hit.hit || hit.distance > 2.2f)
        return false;
    Vector3 across = Vector3Subtract(SCREEN_TR, SCREEN_TL), down = Vector3Subtract(SCREEN_BL, SCREEN_TL);
    Vector3 rel = Vector3Subtract(hit.point, SCREEN_TL);
    at->x = Vector3DotProduct(rel, across) / Vector3LengthSqr(across) * UI_W;
    at->y = Vector3DotProduct(rel, down) / Vector3LengthSqr(down) * UI_H;
    return true;
}

/* ------------------------------------------------------------------ */
/* The UI and its texture.                                             */

struct screen {
    XrUi *ui;
    uint32_t *pixels, *rows; /* the frame, and the damaged rows for upload */
    Texture2D texture;
};

static struct screen screen_new(const char *template) {
    struct screen s = {0};
    s.ui = xr_new(NULL, 0);
    if (!s.ui)
        fail("xr_new");
    if (xr_load_template(s.ui, template) != 0)
        fail("xr_load_template");
    set_model(s.ui);
    xr_set_size(s.ui, UI_W, UI_H);
    /* xerune doesn't write the top byte, which is alpha in raylib's RGBA:
     * opaque once and for all. */
    s.pixels = malloc(UI_W * UI_H * sizeof *s.pixels);
    s.rows = malloc(UI_W * UI_H * sizeof *s.rows);
    for (int i = 0; i < UI_W * UI_H; i++)
        s.pixels[i] = 0xff000000;
    return s;
}

static void screen_texture(struct screen *s) {
    Image image = {s->pixels, UI_W, UI_H, 1, PIXELFORMAT_UNCOMPRESSED_R8G8B8A8};
    s->texture = LoadTextureFromImage(image);
    GenTextureMipmaps(&s->texture);
    SetTextureFilter(s->texture, TEXTURE_FILTER_TRILINEAR);
    SetTextureFilter(s->texture, TEXTURE_FILTER_ANISOTROPIC_8X);
}

/* Moves the UI's time on and uploads what it redrew. */
static void screen_update(struct screen *s, double now_ms) {
    xr_tick(s->ui, (uint64_t)now_ms);
    XrRect d;
    /* XBGR8888 is R, G, B, X in memory: raylib's RGBA. */
    if (!xr_render(s->ui, s->pixels, UI_W, XR_PIXELS_XBGR8888, &d) || !s->texture.id)
        return;
    for (int y = 0; y < d.height; y++)
        memcpy(s->rows + y * d.width, s->pixels + (d.y + y) * UI_W + d.x, d.width * sizeof *s->rows);
    UpdateTextureRec(s->texture, (Rectangle){d.x, d.y, d.width, d.height}, s->rows);
    GenTextureMipmaps(&s->texture);
}

/* `since_click`: seconds since the last click, which rings out. */
static void crosshair(int w, int h, bool on_screen, float scale, float since_click) {
    Vector2 c = {w / 2.0f, h / 2.0f};
    if (since_click >= 0 && since_click < 0.35f) {
        float k = since_click / 0.35f;
        DrawRing(c, (5 + 14 * k) * scale, (6.5f + 14 * k) * scale, 0, 360, 32, (Color){255, 255, 255, (unsigned char)(220 * (1 - k))});
    }
    if (on_screen) {
        DrawCircleV(c, 4 * scale, (Color){255, 255, 255, 230});
        DrawCircleV(c, 2.5f * scale, (Color){40, 40, 40, 230});
    } else {
        DrawCircleV(c, 2.5f * scale, (Color){255, 255, 255, 150});
    }
}

/* ------------------------------------------------------------------ */
/* A scripted visit, for --record.                                     */

struct key {
    float t;
    Vector3 pos, look;
};

/* The velocity through key `i` (units per second): from its neighbors,
 * so the camera moves on through keys, but zero where it holds still (the
 * same value twice in a row: aiming at a button for a click). */
static Vector3 key_velocity(const struct key *keys, int n, int i, bool look) {
    if (i == 0 || i == n - 1)
        return Vector3Zero();
    Vector3 prev = look ? keys[i - 1].look : keys[i - 1].pos;
    Vector3 here = look ? keys[i].look : keys[i].pos;
    Vector3 next = look ? keys[i + 1].look : keys[i + 1].pos;
    if (Vector3Equals(prev, here) || Vector3Equals(here, next))
        return Vector3Zero();
    return Vector3Scale(Vector3Subtract(next, prev), 1.0f / (keys[i + 1].t - keys[i - 1].t));
}

/* A cubic Hermite curve through the keys. */
static Vector3 hermite(const struct key *keys, int n, int i, float s, bool look) {
    float h = keys[i + 1].t - keys[i].t;
    Vector3 p0 = look ? keys[i].look : keys[i].pos, p1 = look ? keys[i + 1].look : keys[i + 1].pos;
    Vector3 m0 = Vector3Scale(key_velocity(keys, n, i, look), h), m1 = Vector3Scale(key_velocity(keys, n, i + 1, look), h);
    float s2 = s * s, s3 = s2 * s;
    Vector3 r = Vector3Scale(p0, 2 * s3 - 3 * s2 + 1);
    r = Vector3Add(r, Vector3Scale(m0, s3 - 2 * s2 + s));
    r = Vector3Add(r, Vector3Scale(p1, -2 * s3 + 3 * s2));
    return Vector3Add(r, Vector3Scale(m1, s3 - s2));
}

static void script_camera(const struct key *keys, int n, float t, Camera3D *cam) {
    int i = 0;
    while (i < n - 2 && t > keys[i + 1].t)
        i++;
    float s = Clamp((t - keys[i].t) / (keys[i + 1].t - keys[i].t), 0, 1);
    cam->position = hermite(keys, n, i, s, false);
    cam->target = hermite(keys, n, i, s, true);
}

/* ------------------------------------------------------------------ */

static void write_ppm(const char *path, const uint32_t *pixels) {
    FILE *f = fopen(path, "wb");
    if (!f) {
        perror(path);
        exit(1);
    }
    fprintf(f, "P6\n%d %d\n255\n", UI_W, UI_H);
    for (int i = 0; i < UI_W * UI_H; i++) {
        unsigned char rgb[3] = {pixels[i] & 0xff, (pixels[i] >> 8) & 0xff, (pixels[i] >> 16) & 0xff};
        fwrite(rgb, 1, 3, f);
    }
    fclose(f);
}

static Camera3D camera(void) {
    Camera3D cam = {0};
    cam.position = (Vector3){1.6f, 1.6f, 3.2f};
    cam.target = SCREEN_C;
    cam.up = (Vector3){0, 1, 0};
    cam.fovy = 60;
    cam.projection = CAMERA_PERSPECTIVE;
    return cam;
}

static void draw_scene(Camera3D cam, struct screen *s, int w, int h, bool on_screen, float scale, float since_click) {
    ClearBackground((Color){10, 10, 14, 255});
    BeginMode3D(cam);
    draw_room(s->texture);
    EndMode3D();
    crosshair(w, h, on_screen, scale, since_click);
}

static int record(const char *template, const char *out) {
    const int w = 2560, h = 1440, fps = 60; /* 2x, scaled down by ffmpeg */
    SetConfigFlags(FLAG_WINDOW_HIDDEN);
    SetTraceLogLevel(LOG_WARNING);
    InitWindow(320, 180, "pc (recording)");
    if (!IsWindowReady())
        return 1;
    struct screen s = screen_new(template);
    screen_texture(&s);
    RenderTexture2D target = LoadRenderTexture(w, h);

    const char *ext = strrchr(out, '.');
    const char *encode = ext && strcmp(ext, ".webp") == 0
        /* For the README: small (about 2 MB). Encode the frames, not an mp4:
         * its grain turns into blocks in the webp. */
        ? "-vf vflip,fps=24,scale=720:-1:flags=lanczos -c:v libwebp_anim -quality 75 -compression_level 6 -loop 0"
        : "-vf vflip,scale=1280:720:flags=lanczos -c:v libx264 -preset slow -crf 18 -pix_fmt yuv420p";
    char cmd[1024];
    snprintf(cmd, sizeof cmd, "ffmpeg -y -loglevel error -f rawvideo -pix_fmt rgba -s %dx%d -r %d -i - %s '%s'", w, h, fps,
             encode, out);
    FILE *ffmpeg = popen(cmd, "w");
    if (!ffmpeg) {
        perror("ffmpeg");
        return 1;
    }

    /* Buy buttons in UI pixels (`--shot` to find them). */
    Vector3 potion = ui_point(541, 267), bow = ui_point(245, 433), delivery = ui_point(480, 500);
    Vector3 sit = {0, 1.22f, 0.62f};
    /* Where the camera is and what it looks at, when. A key repeated
     * holds still (aiming for a click); the camera moves on through the
     * others. */
    const struct key keys[] = {
        {0.0f, {1.5f, 1.6f, 2.7f}, {0.3f, 1.0f, -0.2f}},
        {1.8f, {0.2f, 1.4f, 1.1f}, SCREEN_C},
        {2.6f, sit, potion},
        {3.2f, sit, potion},
        {3.9f, sit, bow},
        {4.5f, sit, bow},
        {5.4f, {0, 1.24f, 0.7f}, delivery},
        {6.8f, {-0.9f, 1.6f, 1.9f}, {-1.9f, 1.45f, -0.7f}},
        {8.0f, {-1.3f, 1.6f, 2.3f}, {3.4f, 1.1f, 1.5f}},
        {9.4f, {-0.6f, 1.55f, 2.0f}, SCREEN_C},
        {10.8f, {0.05f, 1.3f, 0.95f}, SCREEN_C},
        {11.6f, {0.05f, 1.3f, 0.95f}, SCREEN_C},
    };
    const int n_keys = sizeof keys / sizeof keys[0];
    const float clicks[] = {2.9f, 4.2f};
    const int n_clicks = sizeof clicks / sizeof clicks[0];
    const float end = keys[n_keys - 1].t;

    Camera3D cam = camera();
    bool down = false;
    Vector2 at = {-10, -10};
    for (int frame = 0; frame <= (int)(end * fps); frame++) {
        float t = (float)frame / fps;
        script_camera(keys, n_keys, t, &cam);
        bool on = aim(cam, &at);
        if (on)
            xr_pointer_move(s.ui, at.x, at.y);
        else
            xr_pointer_move(s.ui, -10, -10);
        float since_click = -1;
        for (int c = 0; c < n_clicks; c++) {
            if (t >= clicks[c])
                since_click = t - clicks[c];
            if (!down && t >= clicks[c] && t < clicks[c] + 0.1f)
                xr_pointer_down(s.ui, at.x, at.y), down = true;
            else if (down && t >= clicks[c] + 0.1f && t < clicks[c] + 0.2f)
                xr_pointer_up(s.ui, at.x, at.y), down = false;
        }
        handle_messages(s.ui);
        screen_update(&s, t * 1000.0);

        BeginTextureMode(target);
        draw_scene(cam, &s, w, h, on, 2, since_click);
        EndTextureMode();
        Image image = LoadImageFromTexture(target.texture);
        fwrite(image.data, 1, (size_t)w * h * 4, ffmpeg);
        UnloadImage(image);
        if (frame % fps == 0)
            fprintf(stderr, "\r%.0f / %.0f s", t, end);
    }
    fprintf(stderr, "\n");
    int status = pclose(ffmpeg);
    UnloadRenderTexture(target);
    CloseWindow();
    xr_free(s.ui);
    if (status != 0)
        fprintf(stderr, "ffmpeg failed\n");
    else
        printf("wrote %s\n", out);
    return status != 0;
}

static int play(const char *template) {
    SetConfigFlags(FLAG_MSAA_4X_HINT | FLAG_VSYNC_HINT | FLAG_WINDOW_RESIZABLE);
    InitWindow(1280, 720, "xerune on an in-game PC");
    if (!IsWindowReady())
        return 1;
    DisableCursor();
    struct screen s = screen_new(template);
    screen_texture(&s);

    Camera3D cam = camera();
    float eye = 1.6f; /* standing; C sits */
    bool down = false, was_on = false;
    double clicked = -1;
    Vector2 at = {0};
    while (!WindowShouldClose()) {
        Vector3 before = cam.position;
        UpdateCamera(&cam, CAMERA_FIRST_PERSON);
        if (IsKeyPressed(KEY_C))
            eye = eye > 1.4f ? 1.22f : 1.6f;
        /* Stay in the room, in front of the desk, at eye height. */
        Vector3 p = cam.position;
        p.x = Clamp(p.x, -3.2f, 3.2f);
        p.z = Clamp(p.z, 0.3f, 4.2f);
        p.y += (eye - p.y) * 0.15f;
        Vector3 moved = Vector3Subtract(p, cam.position);
        cam.position = p;
        cam.target = Vector3Add(cam.target, moved);
        (void)before;

        bool on = aim(cam, &at);
        if (on) {
            xr_pointer_move(s.ui, at.x, at.y);
            if (IsMouseButtonPressed(MOUSE_BUTTON_LEFT))
                xr_pointer_down(s.ui, at.x, at.y), down = true, clicked = GetTime();
            float wheel = GetMouseWheelMove();
            if (wheel != 0)
                xr_scroll(s.ui, at.x, at.y, 0, -wheel * 60);
        } else if (was_on) {
            xr_pointer_move(s.ui, -10, -10); /* hover ends */
        }
        if (down && IsMouseButtonReleased(MOUSE_BUTTON_LEFT))
            xr_pointer_up(s.ui, at.x, at.y), down = false;
        was_on = on;

        handle_messages(s.ui);
        screen_update(&s, GetTime() * 1000.0);

        BeginDrawing();
        draw_scene(cam, &s, GetScreenWidth(), GetScreenHeight(), on, 1, clicked < 0 ? -1 : (float)(GetTime() - clicked));
        DrawText("WASD walk   mouse look   C sit/stand   click the screen   Esc quit", 12, 10, 18, (Color){200, 200, 210, 160});
        DrawText(TextFormat("%ld gold", gold), 12, 34, 18, (Color){245, 197, 66, 200});
        EndDrawing();
    }
    CloseWindow();
    xr_free(s.ui);
    return 0;
}

int main(int argc, char **argv) {
    const char *template = "../ui/market.html";
    if (argc > 2 && strcmp(argv[1], "--record") == 0)
        return record(template, argv[2]);
    if (argc > 2 && strcmp(argv[1], "--shot") == 0) {
        struct screen s = screen_new(template);
        screen_update(&s, 0);
        bool changed = false;
        for (int i = 4; i < argc; i++)
            changed |= update(argv[i]);
        if (changed)
            set_model(s.ui);
        screen_update(&s, argc > 3 ? atof(argv[3]) : 0);
        write_ppm(argv[2], s.pixels);
        return 0;
    }
    return play(template);
}
