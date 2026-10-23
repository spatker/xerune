/* xerune: HTML/CSS user interfaces drawn into a pixel buffer.
 *
 * Generated from xerune_capi/src/lib.rs by cbindgen; don't edit. To update:
 *   XERUNE_UPDATE_HEADER=1 cargo test -p xerune_capi --test header
 *
 * Usage:
 *   XrUi *ui = xr_new(NULL, 0);                    // bundled Roboto
 *   xr_load_template(ui, "ui/hud.html");
 *   xr_set_model(ui, "{\"score\": 0}");
 *   xr_set_size(ui, 1280, 720);
 *   every frame:
 *     forward input: xr_pointer_move/down/up, xr_scroll, xr_key_down, ...
 *     for (const char *m; (m = xr_next_message(ui));) { handle m, xr_set_model(...) }
 *     xr_tick(ui, now_ms);
 *     if (xr_render(ui, pixels, stride, XR_PIXELS_XBGR8888, &damage)) upload damage
 *   xr_free(ui);
 *
 * Or an app, which shows itself (DRM/KMS, fbdev, a desktop window) and
 * reads the input devices on a thread of its own:
 *   XrApp *app = xr_app_new(NULL, 0);
 *   xr_app_load_template(app, "ui/main.html");
 *   xr_app_set_model(app, json);
 *   xr_app_start(app, "Player", 800, 480);
 *   when xr_app_message_fd(app) is readable (poll, epoll, any event loop):
 *     for (const char *m; (m = xr_app_next_message(app));) { handle m, xr_app_set_model(...) }
 *     for (const char *e; (e = xr_app_next_error(app));) { report e }
 *     if (!xr_app_running(app)) the app ended
 *   xr_app_free(app);
 *
 * Functions that can fail return -1 (or false, or NULL) and set
 * xr_last_error(). Strings are UTF-8 and NUL-terminated.
 */

#ifndef XERUNE_H
#define XERUNE_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

// Pixels are 32-bit words `0xXXRRGGBB` (blue in the low byte; the top
// byte is not written): DRM/fbdev XRGB8888, `GL_BGRA` with
// `GL_UNSIGNED_INT_8_8_8_8_REV`, SDL_PIXELFORMAT_XRGB8888.
#define XR_PIXELS_XRGB8888 0

// Pixels are 32-bit words `0xXXBBGGRR` (red in the low byte; the top byte
// is not written): in memory on little-endian machines R, G, B, X, as
// `GL_RGBA` / `GL_UNSIGNED_BYTE` reads them.
#define XR_PIXELS_XBGR8888 1

// A UI that shows itself on the display and reads the input devices, on a
// thread of its own. The program sets its model and takes its messages,
// from one thread at a time. Create one with `xr_app_new`, free it with
// `xr_app_free`.
typedef struct XrApp XrApp;

// A UI: a template, the model it shows, and its state between frames
// (scroll positions, animations, the previous frame). Create one with
// `xr_new`, free it with `xr_free`.
//
// A UI is used from one thread at a time.
typedef struct XrUi XrUi;

// A font: TrueType or OpenType data for normal text and, optionally, for
// bold text. xerune copies the data; it needn't outlive the call.
typedef struct {
  // The font for normal text.
  const uint8_t *regular;
  size_t regular_len;
  // The font for bold text, or NULL to draw bold text with `regular`.
  const uint8_t *bold;
  size_t bold_len;
} XrFont;

// A rectangle in pixels.
typedef struct {
  int32_t x;
  int32_t y;
  int32_t width;
  int32_t height;
} XrRect;

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

// The version of xerune, e.g. "0.1.0".
const char *xr_version(void);

// The error of the last call on this thread that failed, as text. It
// stays until the next call on this thread fails.
const char *xr_last_error(void);

// A new UI drawing text with `fonts`: the first is the text font, the
// others fallbacks for characters it lacks (e.g. a CJK font). With NULL
// (or `count` 0) text is drawn with the bundled Roboto. NULL on an error.
//
// The UI shows nothing until it has a template (`xr_load_template`) and a
// size (`xr_set_size`).
//
// # Safety
// `fonts` must be NULL or point to `count` valid `XrFont`s.
XrUi *xr_new(const XrFont *fonts, size_t count);

// Frees `ui` (NULL is fine). Strings it returned are gone with it.
//
// # Safety
// `ui` must be NULL or from `xr_new`, and not freed already.
void xr_free(XrUi *ui);

// Shows the template in the file at `path` from now on (UTF-8 HTML with
// `{{ }}`, `{% if %}`, `{% for %}`, `{% include %}`; plain HTML is a
// template too). Includes and images are found relative to its directory.
// Loading a template again (an edited file) keeps the UI's state.
//
// 0 on success; -1 on an error (`xr_last_error`), and the UI shows what
// it showed before.
//
// # Safety
// `ui` must be from `xr_new`; `path` a NUL-terminated string.
int32_t xr_load_template(XrUi *ui, const char *path);

// Like `xr_load_template`, with the template's text in `source`; includes
// and images are found relative to `base_dir` (NULL: the working
// directory).
//
// # Safety
// `ui` must be from `xr_new`; `source` and `base_dir` NUL-terminated
// strings (`base_dir` may be NULL).
int32_t xr_load_template_source(XrUi *ui, const char *source, const char *base_dir);

// Sets the model the template shows: a JSON object whose fields the
// template reads (`{{ score }}`, `{% for item in items %}`). Integers are
// 64-bit, `null` is `None` (`{% if let Some(name) = player %}`), and
// conditions must be `true`/`false`. Set the whole model each time; the
// view is rebuilt from it.
//
// 0 on success; -1 on an error (not JSON, or the template reads a field
// it lacks: `xr_last_error`), and the UI shows what it showed before.
//
// # Safety
// `ui` must be from `xr_new`; `json` a NUL-terminated string.
int32_t xr_set_model(XrUi *ui, const char *json);

// Sets the size of the UI in pixels: the size of the buffer `xr_render`
// draws into, and the viewport of the template's CSS (`@media`, `vw`,
// `vh`).
//
// # Safety
// `ui` must be from `xr_new`.
void xr_set_size(XrUi *ui, uint32_t width, uint32_t height);

// Moves the UI's time on to `now_ms`, a monotonic time in milliseconds
// (any origin; the first call is time 0), and does what is due: CSS
// animations and transitions step, loaded images appear. Call it once a
// frame, before `xr_render`.
//
// Returns true if the UI changed and should be rendered.
//
// # Safety
// `ui` must be from `xr_new`.
bool xr_tick(XrUi *ui, uint64_t now_ms);

// Draws what changed since the last call into `pixels`, a buffer of the
// UI's size (`xr_set_size`) whose rows are `stride` pixels apart, in
// `format` (`XR_PIXELS_...`). The buffer must hold the frame the last call
// drew (call `xr_repaint_all` when it doesn't). The first call, and the
// first after a resize, draw everything.
//
// Returns true if it drew anything; then `damage` (if not NULL) is the
// area it drew, e.g. the part of a texture to upload.
//
// # Safety
// `ui` must be from `xr_new`; `pixels` must hold `stride * height`
// 32-bit pixels; `damage` must be NULL or writable.
bool xr_render(XrUi *ui, uint32_t *pixels, size_t stride, uint32_t format, XrRect *damage);

// Makes the next `xr_render` draw everything: for a buffer that doesn't
// hold the last frame (a new one, or one shared with other drawing).
//
// # Safety
// `ui` must be from `xr_new`.
void xr_repaint_all(XrUi *ui);

// The pixels of the view's `<canvas id="...">`, for the program to draw
// into: `*width` x `*height` pixels of 4 bytes, R, G, B, A in memory
// (straight alpha; transparent pixels show what is behind the canvas),
// rows `*width * 4` bytes apart. The next `xr_render` draws the canvas,
// scaled to the element's box. NULL if the view has no such canvas
// (`xr_last_error`).
//
// The canvas's size is its `width` and `height` attributes (or its CSS
// size in px; 200 x 200 without either) when the view first showed it.
// The canvas keeps its pixels between frames; the pointer is valid until
// the next call on `ui`, so call this each frame the canvas changes.
//
// # Safety
// `ui` must be from `xr_new`; `id` a NUL-terminated string; `width` and
// `height` NULL or writable.
uint8_t *xr_canvas(XrUi *ui, const char *id, uint32_t *width, uint32_t *height);

// The pointer (mouse or touch, in pixels) moved to (`x`, `y`): hover,
// dragging a slider, scrolling by dragging while down. Returns true if
// the UI changed.
//
// # Safety
// `ui` must be from `xr_new`.
bool xr_pointer_move(XrUi *ui, float x, float y);

// The pointer went down at (`x`, `y`). Returns true if the UI changed.
//
// # Safety
// `ui` must be from `xr_new`.
bool xr_pointer_down(XrUi *ui, float x, float y);

// The pointer went up at (`x`, `y`): a click if it didn't move far since
// it went down, which sends the element's `data-on-click` message
// (`xr_next_message`). Returns true if the UI changed.
//
// # Safety
// `ui` must be from `xr_new`.
bool xr_pointer_up(XrUi *ui, float x, float y);

// Scrolls the scrollable element under (`x`, `y`) by (`dx`, `dy`) pixels
// (a mouse wheel: positive `dy` scrolls down). Returns true if the UI
// changed.
//
// # Safety
// `ui` must be from `xr_new`.
bool xr_scroll(XrUi *ui, float x, float y, float dx, float dy);

// Scrolls the element with the `id` attribute into view, in its nearest
// scrollable ancestor (as the DOM's `element.scrollIntoView()`): e.g. the
// row a D-pad selected. Nothing happens if there is no such element.
// Returns true if the UI changed.
//
// # Safety
// `ui` must be from `xr_new`; `id` a NUL-terminated string.
bool xr_scroll_into_view(XrUi *ui, const char *id);

// Whether (`x`, `y`) is on the UI rather than on the bare page: on an
// element that takes clicks or any element inside `<body>`. A game
// drawing the UI over its scene sends the pointer to the UI only where
// this is true.
//
// # Safety
// `ui` must be from `xr_new`.
bool xr_hit(XrUi *ui, float x, float y);

// A key went down. `key` is the key's code as browsers name it
// (`KeyboardEvent.code`: "KeyA", "Digit1", "Enter", "Escape",
// "ArrowUp", "Space", ...). The UI sends the message "keydown:<key>".
// Returns true if the UI changed.
//
// # Safety
// `ui` must be from `xr_new`; `key` a NUL-terminated string.
bool xr_key_down(XrUi *ui, const char *key);

// A key went up; the UI sends the message "keyup:<key>". See
// `xr_key_down`.
//
// # Safety
// `ui` must be from `xr_new`; `key` a NUL-terminated string.
bool xr_key_up(XrUi *ui, const char *key);

// Text was typed (UTF-8). If a text `<input id="name">` has the focus
// (it was clicked), the UI sends the message "name:text:<text>"; the
// model sets the input's `value`. Returns true if the UI changed.
//
// # Safety
// `ui` must be from `xr_new`; `text` a NUL-terminated string.
bool xr_text_input(XrUi *ui, const char *text);

// The next message the UI sent, oldest first, or NULL when there is none.
// Messages are the `data-on-click` (and `data-on-input`, `data-on-change`,
// `data-on-hover`, `data-on-transitionend`, ...) attributes of the
// elements, e.g. "buy:3" for `data-on-click="buy:{{ item.id }}"`; ranges
// append ":<value>". Handle each by updating the model (`xr_set_model`).
//
// The string is valid until the next call of `xr_next_message` or
// `xr_free`.
//
// # Safety
// `ui` must be from `xr_new`.
const char *xr_next_message(XrUi *ui);

// Waits up to `timeout_ms` for the images the view shows to load (they
// load in the background and appear on a later `xr_tick`): for a first
// frame or a screenshot that should have them. Returns true if all
// loaded (or failed) in time.
//
// # Safety
// `ui` must be from `xr_new`.
bool xr_wait_for_images(XrUi *ui, uint32_t timeout_ms);

// A new app drawing text with `fonts` (as `xr_new`). NULL on an error.
//
// It shows nothing until it is started (`xr_app_start`), usually after
// `xr_app_load_template` and `xr_app_set_model`.
//
// # Safety
// `fonts` must be NULL or point to `count` valid `XrFont`s.
XrApp *xr_app_new(const XrFont *fonts, size_t count);

// Ends the app if it runs (the display is left as it was) and frees it
// (NULL is fine). Strings it returned are gone with it.
//
// # Safety
// `app` must be NULL or from `xr_app_new`, and not freed already.
void xr_app_free(XrApp *app);

// Shows the template in the file at `path` (as `xr_load_template`),
// before the app starts or while it runs.
//
// 0 on success; -1 if the template can't be read or parsed
// (`xr_last_error`). Whether it fits the model is found on the app's
// thread: see `xr_app_next_error`.
//
// # Safety
// `app` must be from `xr_app_new`; `path` a NUL-terminated string.
int32_t xr_app_load_template(XrApp *app, const char *path);

// Like `xr_app_load_template`, with the template's text in `source`;
// includes and images are found relative to `base_dir` (NULL: the working
// directory).
//
// # Safety
// `app` must be from `xr_app_new`; `source` and `base_dir` NUL-terminated
// strings (`base_dir` may be NULL).
int32_t xr_app_load_template_source(XrApp *app, const char *source, const char *base_dir);

// Sets the model the template shows (a JSON object, as `xr_set_model`),
// before the app starts or while it runs. Set the whole model each time.
// The app takes it on its own thread soon after; when the model is set
// again before that, it shows only the latest.
//
// 0 on success; -1 if the app has ended (`xr_last_error`). An error in
// the model (not JSON, a field the template lacks) is found on the app's
// thread: see `xr_app_next_error`; the app shows what it showed before.
//
// # Safety
// `app` must be from `xr_app_new`; `json` a NUL-terminated string.
int32_t xr_app_set_model(XrApp *app, const char *json);

// Starts the app on a thread of its own: it shows the template and model
// set so far on the display and reads the input devices. The display is
// picked when it starts: a window of `width` x `height` titled `title`
// (NULL: "xerune") in a desktop session, else DRM/KMS at the display's
// size, else the fbdev framebuffer; the environment variable
// `XERUNE_BACKEND=winit|drm|fbdev` picks one. Only the displays the
// library was built with are there (Cargo features `drm`, `linuxfb`,
// `winit`). With `XERUNE_SNAPSHOT=out.png` it renders a screenshot
// instead and ends.
//
// An app starts once. 0 on success; -1 if it can't start (no display,
// no template, a model that doesn't fit it: `xr_last_error`).
//
// # Safety
// `app` must be from `xr_app_new`; `title` NULL or a NUL-terminated
// string.
int32_t xr_app_start(XrApp *app, const char *title, uint32_t width, uint32_t height);

// Scrolls the element with the `id` attribute into view (as
// `xr_scroll_into_view`), on the app's thread, after the models set
// before: set the model that selects a row, then scroll to it.
//
// 0 on success; -1 if the app isn't running (`xr_last_error`).
//
// # Safety
// `app` must be from `xr_app_new`; `id` a NUL-terminated string.
int32_t xr_app_scroll_into_view(XrApp *app, const char *id);

// Whether the app runs: false before `xr_app_start` and once it ended (its
// window was closed, or its display failed: `xr_app_next_error`).
//
// # Safety
// `app` must be from `xr_app_new`.
bool xr_app_running(XrApp *app);

// A file descriptor that is readable while messages or errors wait
// (`xr_app_next_message`, `xr_app_next_error`), and once when the app
// ends: for `poll`, `epoll` or any event loop that watches descriptors.
// Taking everything that waits makes it unreadable again. The app owns
// it: don't read or close it. -1 where there is none (only Linux has
// one).
//
// # Safety
// `app` must be from `xr_app_new`.
int32_t xr_app_message_fd(XrApp *app);

// The next message the UI sent (as `xr_next_message`), oldest first, or
// NULL when there is none. Handle each by setting the model
// (`xr_app_set_model`).
//
// The string is valid until the next call of `xr_app_next_message` or
// `xr_app_free`.
//
// # Safety
// `app` must be from `xr_app_new`.
const char *xr_app_next_message(XrApp *app);

// The next error found on the app's thread, oldest first, or NULL when
// there is none: a model or template that doesn't fit (the app shows what
// it showed before), or why the app ended.
//
// The string is valid until the next call of `xr_app_next_error` or
// `xr_app_free`.
//
// # Safety
// `app` must be from `xr_app_new`.
const char *xr_app_next_error(XrApp *app);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* XERUNE_H */
