/* The shop as an app: xerune shows it by itself (a window on a desktop,
 * DRM/KMS or fbdev on a device) and reads the input devices, on a thread of
 * its own. This program only keeps the model: it waits in poll() for the
 * view's messages and for its own events (here, gold that comes in each
 * second), and sets the model when it changed.
 *
 *   make run-app           (or: ./app [template])
 *
 * Build the library with the displays to use, e.g.
 *   cargo build --release -p xerune_capi --features winit,drm,linuxfb
 * XERUNE_BACKEND=winit|drm|fbdev picks one; with XERUNE_SNAPSHOT=out.png
 * (and XERUNE_SNAPSHOT_MESSAGES="buy:2") the app renders one frame and
 * ends, as CI runs it.
 *
 * Linux only: the messages come through a file descriptor.
 */

#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include "xerune.h"

struct item {
    int id;
    const char *name;
    long price;
    bool owned;
};

struct model {
    long gold;
    struct item items[3];
    const char *last_purchase;
};

static void fail(const char *what) {
    fprintf(stderr, "%s: %s\n", what, xr_last_error());
    exit(1);
}

/* As in shop.c: the model as JSON. */
static void set_model(XrApp *app, const struct model *model) {
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
    snprintf(json + n, sizeof json - n, ", \"owned_share\": %f}", owned / 3.0);
    /* Fails only once the app has ended. */
    xr_app_set_model(app, json);
}

/* Handles a message of the view: "buy:<id>", or a key ("keydown:Escape")
 * the app read. Returns false to quit. */
static bool update(struct model *model, const char *message) {
    if (strcmp(message, "keydown:Escape") == 0)
        return false;
    if (strncmp(message, "buy:", 4) != 0)
        return true;
    int id = atoi(message + 4);
    for (int i = 0; i < 3; i++) {
        struct item *item = &model->items[i];
        if (item->id == id && !item->owned && item->price <= model->gold) {
            model->gold -= item->price;
            item->owned = true;
            model->last_purchase = item->name;
        }
    }
    return true;
}

static long now_ms(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec * 1000 + t.tv_nsec / 1000000;
}

int main(int argc, char **argv) {
    const char *template = argc > 1 ? argv[1] : "../ui/shop.html";

    XrApp *app = xr_app_new(NULL, 0);
    if (!app)
        fail("xr_app_new");
    if (xr_app_load_template(app, template) != 0)
        fail("xr_app_load_template");
    struct model model = {
        .gold = 100,
        .items = {
            {1, "Iron sword", 40, false},
            {2, "Health potion", 15, false},
            {3, "Dragon shield", 400, false},
        },
    };
    set_model(app, &model);
    if (xr_app_start(app, "Shop", 640, 400) != 0)
        fail("xr_app_start");

    struct pollfd fds[] = {{.fd = xr_app_message_fd(app), .events = POLLIN}};
    long next_gold = now_ms() + 1000;
    bool running = true;
    while (running) {
        /* A real program polls its other descriptors here too (sockets,
         * timers, its own event loop's). */
        long wait = next_gold - now_ms();
        if (poll(fds, 1, wait > 0 ? (int)wait : 0) < 0)
            break;
        bool changed = false;
        for (const char *message; (message = xr_app_next_message(app));) {
            printf("message: %s\n", message);
            running &= update(&model, message);
            changed = true;
        }
        for (const char *error; (error = xr_app_next_error(app));)
            fprintf(stderr, "xerune: %s\n", error);
        if (!xr_app_running(app))
            break;
        if (now_ms() >= next_gold) {
            model.gold += 5;
            next_gold += 1000;
            changed = true;
        }
        if (changed)
            set_model(app, &model);
    }
    printf("gold left: %ld\n", model.gold);
    xr_app_free(app);
    /* CI runs it with XERUNE_SNAPSHOT_MESSAGES="buy:2". */
    if (getenv("XERUNE_SNAPSHOT") && !model.last_purchase) {
        fprintf(stderr, "the message bought nothing\n");
        return 1;
    }
    return 0;
}
