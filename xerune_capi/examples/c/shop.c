/* A shop drawn by xerune, driven from C through its C API: render, click
 * Buy, handle the message by updating the model, render again, and write
 * the frame to a PPM file. A game does the same each frame, with real
 * input, and uploads the damaged area to a texture.
 *
 *   make run                 (or: ./shop [template] [out.ppm])
 *
 * The template is ../ui/shop.html, shared with the Zig sample.
 *
 * Nothing here is C-specific: any language that calls C functions
 * (C++, Zig, Rust, Go, C#, Python, ...) uses the API the same way.
 */

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "xerune.h"

enum { WIDTH = 640, HEIGHT = 400 };

/* The model the template shows; it goes to xerune as JSON. */
struct item {
    int id;
    const char *name;
    long price;
    bool owned;
};

struct model {
    long gold;
    struct item items[3];
    /* NULL until something is bought (`{% if let Some(..) %}`). */
    const char *last_purchase;
};

static void fail(const char *what) {
    fprintf(stderr, "%s: %s\n", what, xr_last_error());
    exit(1);
}

/* The model as JSON: the fields the template reads. A real program uses
 * its JSON library; these strings need no escaping. */
static void set_model(XrUi *ui, const struct model *model) {
    char json[1024];
    size_t n = 0;
    int owned = 0;
    n += snprintf(json + n, sizeof json - n, "{\"gold\": %ld, \"items\": [", model->gold);
    for (int i = 0; i < 3; i++) {
        const struct item *item = &model->items[i];
        owned += item->owned;
        n += snprintf(json + n, sizeof json - n, "%s{\"id\": %d, \"name\": \"%s\", \"price\": %ld, \"owned\": %s}",
                      i ? ", " : "", item->id, item->name, item->price, item->owned ? "true" : "false");
    }
    if (model->last_purchase)
        n += snprintf(json + n, sizeof json - n, "], \"last_purchase\": \"%s\"", model->last_purchase);
    else
        n += snprintf(json + n, sizeof json - n, "], \"last_purchase\": null");
    /* The template has no functions to call: computed values go into the
     * model too. */
    snprintf(json + n, sizeof json - n, ", \"owned_share\": %f}", owned / 3.0);
    if (xr_set_model(ui, json) != 0)
        fail("xr_set_model");
}

/* Presses and releases the pointer at (x, y). */
static void click(XrUi *ui, float x, float y) {
    xr_pointer_move(ui, x, y);
    xr_pointer_down(ui, x, y);
    xr_pointer_up(ui, x, y);
}

/* Handles a message of the view: "buy:<id>". */
static void update(struct model *model, const char *message) {
    if (strncmp(message, "buy:", 4) != 0)
        return;
    int id = atoi(message + 4);
    for (int i = 0; i < 3; i++) {
        struct item *item = &model->items[i];
        if (item->id == id && !item->owned && item->price <= model->gold) {
            model->gold -= item->price;
            item->owned = true;
            model->last_purchase = item->name;
        }
    }
}

static void write_ppm(const char *path, const uint32_t *pixels) {
    FILE *f = fopen(path, "wb");
    if (!f) {
        perror(path);
        exit(1);
    }
    fprintf(f, "P6\n%d %d\n255\n", WIDTH, HEIGHT);
    for (int i = 0; i < WIDTH * HEIGHT; i++) {
        /* XR_PIXELS_XRGB8888: 0xXXRRGGBB. */
        uint8_t rgb[3] = {(uint8_t)(pixels[i] >> 16), (uint8_t)(pixels[i] >> 8), (uint8_t)pixels[i]};
        fwrite(rgb, 1, 3, f);
    }
    fclose(f);
}

int main(int argc, char **argv) {
    const char *template = argc > 1 ? argv[1] : "../ui/shop.html";
    const char *out = argc > 2 ? argv[2] : "shop.ppm";

    printf("xerune %s\n", xr_version());
    XrUi *ui = xr_new(NULL, 0); /* the bundled Roboto */
    if (!ui)
        fail("xr_new");
    if (xr_load_template(ui, template) != 0)
        fail("xr_load_template");

    struct model model = {
        .gold = 100,
        .items = {
            {1, "Iron sword", 40, false},
            {2, "Health potion", 15, false},
            {3, "Dragon shield", 400, false},
        },
    };
    set_model(ui, &model);
    xr_set_size(ui, WIDTH, HEIGHT);

    uint32_t *pixels = calloc(WIDTH * HEIGHT, sizeof *pixels);
    XrRect damage;
    uint64_t now_ms = 0;
    xr_tick(ui, now_ms);
    if (!xr_render(ui, pixels, WIDTH, XR_PIXELS_XRGB8888, &damage))
        fail("xr_render");
    printf("first frame: %dx%d\n", damage.width, damage.height);

    /* Buy the potion: its Buy button is in the second row. */
    click(ui, 573, 146);
    for (const char *message; (message = xr_next_message(ui));) {
        printf("message: %s\n", message);
        update(&model, message);
    }
    set_model(ui, &model);

    /* A few frames, as a game loop would run them. */
    for (int frame = 0; frame < 20; frame++) {
        now_ms += 16;
        xr_tick(ui, now_ms);
        if (xr_render(ui, pixels, WIDTH, XR_PIXELS_XRGB8888, &damage))
            printf("frame at %llu ms: redrew %dx%d at (%d, %d)\n", (unsigned long long)now_ms, damage.width,
                   damage.height, damage.x, damage.y);
    }
    printf("gold left: %ld\n", model.gold);
    /* CI runs the sample: the click must have reached the Buy button. */
    if (!model.last_purchase) {
        fprintf(stderr, "the click bought nothing\n");
        return 1;
    }

    write_ppm(out, pixels);
    printf("wrote %s\n", out);
    free(pixels);
    xr_free(ui);
    return 0;
}
