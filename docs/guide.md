# Xerune Quick Start Guide

This guide walks you through building your first application using Xerune, styling it with CSS, using custom canvases, and deploying to embedded backends.

---

## 1. Minimal Application Setup

Xerune applications follow the Elm/MVU (Model-View-Update) pattern. For a complete, runnable reference implementation, see [`examples/todo`](../examples/todo/).

A complete application consists of:

### 1. Application State & Messages

Define your model struct and message enum:

```rust
use xerune::{Context, Model, XeruneMessage, XeruneTemplate};

#[derive(Default, XeruneTemplate)]
#[template(path = "todo_list.html")]
pub struct TodoList {
    items: Vec<TodoItem>,
    active_item: usize,
    new_item_title: String,
}

#[derive(Debug, Clone, XeruneMessage)]
pub enum TodoMsg {
    Toggle(usize),
    Remove(usize),
    Add,
}

impl Model for TodoList {
    type Message = TodoMsg;

    fn update(&mut self, msg: Self::Message, _ctx: &mut Context<Self::Message>) {
        match msg {
            TodoMsg::Toggle(index) => {
                if let Some(item) = self.items.get_mut(index) {
                    item.completed = !item.completed;
                }
            }
            TodoMsg::Remove(index) => {
                if index < self.items.len() {
                    self.items.remove(index);
                }
            }
            TodoMsg::Add => { /* ... */ }
        }
    }
}
```

### 2. Declarative Template Layout

Templates are precompiled at compile time via `#[derive(XeruneTemplate)]` and bind interaction handlers via `data-on-click`:

```html
<div data-on-click="add" class="add-btn">Add</div>

{% for item in items %}
<div class="todo-item" data-on-click="toggle:{{ loop.index0 }}">
    <span>{{ item.title }}</span>
    <div data-on-click="remove:{{ loop.index0 }}" class="remove-btn">x</div>
</div>
{% endfor %}
```

**Element identity.** Every update rebuilds the view, and xerune finds "the same element" in the new build by a key made from its place in the template and, in a `{% for %}`, the iteration's index. State that outlives a rebuild, like which element is hovered, follows that key. When items are inserted, removed or reordered, give each item a `data-key` that identifies it (`<li data-key="{{ item.id }}">`), so it keeps its identity instead of its position; keys must differ among the loop's items.

`data-on-hover` and `data-on-hover-out` take messages the same way and send them when the pointer enters and leaves the element (entering a child doesn't leave its parent). To only restyle what is under the pointer, `:hover` in CSS needs no messages.

A message is named after its variant in snake_case: `add` for `Add`,
`toggle:` followed by the argument for `Toggle(usize)` (`#[xerune(rename =
"...")]` and `#[xerune(prefix = "...")]` change that). The template builds
the message itself, so `toggle:{{ loop.index0 }}` is type-checked: passing a
`String` where `Toggle` takes a `usize` is a compile error, and the argument
must have the field's exact type or be a reference to it (`&str` works for a
`String` field). Static arguments (`digit:7`) are parsed when the view is
built and an invalid one is logged then. Messages must be `Clone`: an
element keeps its message and sends a copy on each click. The runtime's own
messages (`keydown:Enter`, `<input id>:text:...`) and those from
`Messenger::send`, `XERUNE_SNAPSHOT_MESSAGES` or JavaScript are strings,
parsed with the derived `FromStr`.

To scroll an element into view, give it an `id` and call
`context.scroll_into_view("item-3")` from `update`:
`<div id="item-{{ loop.index0 }}" ...>`.

See [`examples/todo/templates/todo_list.html`](../examples/todo/templates/todo_list.html) for the full layout and CSS.

#### Template syntax

Templates are parsed with [Askama](https://askama.readthedocs.io/)'s parser, so expressions are **Rust**, not Jinja. Everything below is checked at compile time; anything unsupported is a compile error pointing at the template line.

| Construct | Example | Notes |
|---|---|---|
| Expression | `{{ name }}`, `{{ self.total() }}`, `{{ item.title }}` | Bare names are model fields (`self.name`) unless bound by `for` / `if let`. |
| Formatting | `{{ "{:.1}"\|format(volume) }}` | `format` is the **only** filter. For anything else, add a method to your model. |
| Conditionals | `{% if a %}…{% elif b %}…{% else %}…{% endif %}` | |
| Pattern match | `{% if let Some(c) = cover %}{{ c }}{% endif %}` | The value is borrowed; `c` is a reference. |
| Loops | `{% for item in items %}`, `{% for (k, v) in pairs %}`, `{% for i in 0..n %}` | `loop.index` (1-based) and `loop.index0` are available. `{% break %}` / `{% continue %}` work. |
| Includes | `{% include "header.html" %}` | Resolved relative to the including file. |
| Comments | `{# … #}` | |

Operators are Rust's: `&&`, `||`, `!`, `==`, `!=`, `<`, … (**not** `and` / `or` / `not`).
Constructs outside this table are listed below, with what to write instead.

#### How it differs from Jinja, and why

The tags look like Jinja's, but a template is not interpreted at run time:
`#[derive(XeruneTemplate)]` turns it into Rust code (`build_ui`) that is
compiled with your model. That is the reason for every difference below.
On the device there is no template parser, no expression evaluator and no
reflection over the model, so they cost no code size, no startup time and no
per-frame work; and a typo in a field name, a comparison between a number
and a string, or a message argument of the wrong type is a compile error
instead of a blank label on a board in the field.

Most differences are inherited from [Askama](https://askama.readthedocs.io/)
(Rust expressions in Jinja tags), whose parser xerune uses:

| Jinja | xerune | Why |
|---|---|---|
| `{% if a and not b %}` | `{% if a && !b %}` | Expressions are Rust expressions, compiled as written. |
| `{% if items %}` (truthiness) | `{% if !items.is_empty() %}` | A condition must be a `bool` (or `&bool`). Rust has no truthiness, and an implicit one would hide bugs like testing a count instead of a flag. |
| `{% if cover %}…{{ cover }}` (`None` is falsy) | `{% if let Some(c) = cover %}…{{ c }}` | Absent values are `Option`s; the pattern binds the value only where it exists. |
| undefined variable → empty string, `is defined` | compile error | Every name is a model field, a loop variable or an `if let` binding, checked by rustc. |
| `{{ items\|length }}`, `{{ name\|upper }}` | `{{ items.len() }}`, `{{ self.title_upper() }}` | Values are Rust values with Rust methods. Formatting logic lives in model methods, which are ordinary, unit-testable Rust. |
| `{{ "%.1f"\|format(v) }}` | `{{ "{:.1}"\|format(v) }}` | `format` is Rust's `format!`, checked at compile time. |
| `'text'` | `"text"` | `'x'` is a `char`, as in Rust. |
| `x['key']`, dicts | fields, `xs[i]`, `(k, v)` tuples | Model data is typed structs, not dictionaries. |
| `a ~ b` | `"{}{}"\|format(a, b)`, or `{{ a }}{{ b }}` | |

Askama features that xerune leaves out:

| Askama | Status | Instead |
|---|---|---|
| HTML escaping, `\|safe`, `\|escape` | Not needed | `{{ }}` output becomes a text node or an attribute value directly; it is never parsed as HTML, so there is nothing to escape or inject. |
| Filters other than `format`, filter blocks | Left out | A method on the model. |
| `{% extends %}` / `{% block %}` / `{% macro %}` / `{% call %}` / `{% import %}` | Left out | `{% include %}` composes templates; the included file's `<style>` belongs to the page like any other. |
| `{% let %}` / `{% set %}`, `{% match %}` | Not yet | A model method; `{% if let %}` / `{% elif %}` chains. |
| `{% for … if cond %}`, `{% for %}…{% else %}` | Not yet | `{% if %}` in the body; a separate `{% if xs.is_empty() %}`. |
| `loop.first`, `loop.last`, `loop.cycle` | Not yet | `loop.index0 == 0`, `loop.index == xs.len()`. |
| `?`, turbofish, Rust macros in expressions | Left out | A model method. |
| Whitespace control `{%- -%}`, `{%~ ~%}` | Ignored | Text whitespace collapses as CSS `white-space` says, as in a browser. |

"Left out" means not planned; "not yet" means it would fit and could be
added. The language is kept small on purpose: the studio previews templates
in the browser without recompiling, by running the same template grammar
there, and every construct must behave identically in both.

xerune adds a few rules of its own, because the template is HTML that is
laid out and styled at compile time:

- Class names must be literals, switched with `{% if %}` (CSS is matched
  against them when the derive runs).
- Template code inside a start tag must sit inside a quoted attribute value;
  the only tag-level conditional is `{% if c %}checked{% endif %}`.
- `data-on-click="toggle:{{ loop.index0 }}"` and the other `data-on-*`
  attributes build typed messages (see above), and `data-key` gives loop
  items an identity.

**Inside tags**, template code may only appear *inside a quoted attribute value*:

```html
<!-- yes -->
<div class="row {% if selected %}active{% else %}idle{% endif %}">
<img src="{{ cover_url }}">
<p class='{% if title != "" %}has-title{% endif %}'>  <!-- either quote style works -->

<!-- no: compile error -->
<div {% if selected %}class="active"{% endif %}>
```

The one exception is `<input type="checkbox" {% if done %}checked{% endif %}>`.

**Class names must be literals.** CSS is matched against classes at compile time, so `class="{{ name }}"` is rejected; switch between literal classes with `{% if %}` instead.

`#[template(path = "...")]` is looked up, in order: in the crate root, in
its `templates/` directory, and in the workspace root (so crates of a
workspace can share templates). `{% include "..." %}` looks next to the
including template first. A missing template lists every place tried.

For small components and tests, a template can be inline instead of a file: `#[template(source = r#"<p>{{ name }}</p>"#)]`.

### 3. Running the App

`xerune_render` provides the fonts (which the runtime measures text with) and
the software renderer; `App` runs both on a window or a display:

```rust
use xerune::{App, Runtime};
use xerune_render::{Fonts, SoftwareRenderer};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let fonts = Fonts::ttf(include_bytes!("Roboto-Regular.ttf"), Some(include_bytes!("Roboto-Bold.ttf")))?;
    let runtime = Runtime::new(TodoList::default(), fonts.clone());
    App::new(runtime).title("Todo").size(800, 480).run(SoftwareRenderer::new(fonts))?;
    Ok(())
}
```

`App::run` picks the backend when the app starts, among those compiled in:
a window (`winit`) in a desktop session, else DRM/KMS, else the fbdev
framebuffer; `XERUNE_BACKEND=winit|drm|fbdev` overrides it. So one binary
built with `winit,drm,evdev` runs on a desktop and on the device.
`App::on_start` receives a `Messenger` for sending messages from other
threads. `Fonts::bitmap(&xerune::font::DEFAULT_ROBOTO)` uses the bundled
prerasterized fonts instead of TrueType.

```bash
cargo run --release --example todo
```

---

## 2. Using Custom Canvas Buffers

Custom canvas allows you to directly manipulate raw pixel colors:

```rust
// 1. Declare <canvas id="my_canvas"> in HTML template
// 2. Access and draw onto the canvas inside update loop:
fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self::Message>) {
    if let Some(canvas) = ctx.get_canvas_mut("my_canvas", 200, 200) {
        // Clear canvas with white color
        canvas.data.fill(255);
        // Paint a blue pixel at (50, 50)
        let idx = (50 * 200 + 50) * 4;
        canvas.data[idx] = 0;     // Red
        canvas.data[idx + 1] = 0; // Green
        canvas.data[idx + 2] = 255; // Blue
        canvas.data[idx + 3] = 255; // Alpha
        canvas.dirty = true;
    }
}
```

---

## 3. Styling with CSS Stylesheets

Templates can embed global or component stylesheets inside `<style>` blocks:

```html
<style>
    .card {
        background-color: #333333;
        border-radius: 8px;
        padding: 20px;
    }
    #title {
        color: #ffcc00;
        font-size: 24px;
    }
</style>
<div class="card">
    <span id="title">Hello World</span>
</div>
```

```rust
#[derive(XeruneTemplate)]
#[template(path = "my_template.html")]
pub struct MyModel {
    // ...
}
```

CSS is resolved at **compile time**: selectors are matched against the template and each element gets the declarations that apply to it, guarded by any `{% if %}` conditions on its classes. Unsupported properties are ignored with a one-time warning at runtime.

**Selectors:** type (`p`), class (`.card`), id (`#title`), attribute (`[type=checkbox]`, also `^=`, `$=`, `*=`, `~=`, `|=` and the `i` flag), universal (`*`), the combinators (`.list p`, `.list > p`, `h1 + p`, `h1 ~ p`), the structural pseudo-classes (`:first-child`, `:last-child`, `:only-child`, `:nth-child(2n+1 of .x)`, `:nth-last-child()`, the `-of-type` forms, `:empty`, `:root`), `:not()`, `:is()`, `:where()`, `:checked`, the `::before` / `::after` pseudo-elements (also spelled `:before` / `:after`), and `@media` queries (see below). Specificity, `!important` (also in `style` attributes) and invalid selectors work as in browsers: a selector list with one selector xerune doesn't know (`:has()`, a namespace) drops the whole rule, and the template macro warns about it. `:hover` applies to the element under the pointer and its ancestors, and `:active` to them while a touch presses them (mouse buttons only click). On touch screens hover stays on the last tapped element, as in mobile browsers. Moving the pointer rebuilds the view only when what `:hover` / `:active` apply to changes, and only in views whose CSS uses them. In templates, `:hover` on a sibling (`a:hover + b`) doesn't match. `:focus` and the other state pseudo-classes parse but never match. Descendant and child selectors see through `{% for %}` / `{% if %}`. Sibling selectors and the structural pseudo-classes do not: which iteration or branch comes first is only known at runtime, so they never match across a loop or conditional, nor inside a loop's body (`li:nth-child(odd)` in a `{% for %}` doesn't match).

**Properties:**

- Box: `width`, `height`, `min-/max-width`, `min-/max-height`, logical sizes (`inline-size`, `block-size` and their `min-`/`max-` forms), `margin*`, `padding*`, `box-sizing`
- Layout: `display` (`block`, `inline-block`, `inline`, `flex`, `none`), `flex`, `flex-direction`, `flex-wrap`, `flex-flow`, `flex-grow`, `flex-shrink`, `flex-basis`, `justify-content`, `align-items`, `align-self`, `align-content`, `gap`, `row-gap`, `column-gap`, `order`, `position` (`static`, `relative`, `absolute`), `top`/`right`/`bottom`/`left`, `float`, `overflow`, `direction`, `writing-mode`
- Visual: `color`, `background`, `background-color`, `background-image`, `border*` (width, color, style, per side), `border-radius`, `box-shadow`, `accent-color`
- Gradients: `linear-gradient`, `radial-gradient`, `conic-gradient` and the `repeating-` radial/conic forms, with any CSS color, hard stops (`red 0deg 90deg`) and `from <angle>` / `at <position>`. Stop positions take `%` (and angles for conic); `px` stops are not supported. Conic gradients plus `border-radius: 50%` make rings, arcs and dials without a canvas — see the vehicle tab of `examples/showcase`.
- Text: `font-size`, `font-weight`, `font` (size, weight and `/line-height`), `line-height`, `text-align`, `white-space` (`normal`, `nowrap`, `pre`, `pre-wrap`, `pre-line`), `overflow-wrap` / `word-wrap` (`break-word`, `anywhere`), `word-break` (`break-all`), `text-overflow` (`ellipsis`, with `overflow` other than `visible`)
- Animation: `animation` and all `animation-*` longhands, with `@keyframes`

Not supported, among others: `font-family` (fonts are compiled bitmap fonts), `opacity`, `transform`, `transition`, `display: grid`, `z-index`.

**Text layout.** Text wraps to its container like in a browser: lines break at Unicode (UAX #14) break opportunities, so spaces, hyphens and between CJK characters; `text-align` aligns each line. A run of text and `{{ }}` values (`Hello {{ name }}, you have {{ n }} messages`) is one paragraph and wraps as a whole. Limits: inline elements (`<b>`, `<span>`, `<a>`) are separate boxes rather than part of the surrounding paragraph, so text after one starts a new box; there is no OpenType shaping (Roboto's kerning, ligatures, font features such as `font-variant-numeric: proportional-nums`, Arabic or Indic scripts). Roboto's digits are tabular, so right-aligned numbers move as digits with different side bearings appear (a lone `1` ends short of the edge), exactly as in Chromium; Thai, Lao, Khmer and Myanmar break only at spaces. For truncated single lines, use `white-space: nowrap; overflow: hidden; text-overflow: ellipsis` on the element holding the text.

**Page background.** As in browsers, the background of `<html>` (or, if it has none, of `<body>`) paints the whole viewport, behind everything. Without one, the viewport is white, or `#121212` with `html { color-scheme: dark; }`. Every frame repaints from this canvas, so a translucent background never shows the previous frame. If an opaque, square-cornered box already covers the viewport (a full-screen `.app` div, as in the examples), no canvas fill is drawn at all.

As in browsers, `box-sizing` defaults to `content-box`: `width: 800px; padding: 10px` takes up 820px. Add `* { box-sizing: border-box; }` if you prefer the other model.

**Form controls.** `<input type="checkbox">`, `<input type="range">` and `<progress>` look like Chromium's: the same default sizes (13×13, 129×16 and 10em×1em) and margins, a grey-bordered white checkbox that turns into an accent-colored one with a white check mark, and rounded light-grey tracks filled with the accent color. `accent-color` (inherited, like in browsers) recolors them; `width`/`height` resize them as in Chromium (the range's track and thumb keep their size, a progress track is half the control's height). A range takes `min`, `max` (default 0 and 100), `step` (default 1, or `any`) and `value` like in HTML: the value defaults to the middle, is clamped to the range and snaps to a step from `min`. A progress bar shows `value` out of `max` (default 1). As in Chromium, a native-looking checkbox ignores `background`, `border` and `border-radius`; `appearance: none` turns it into a plain box styled by its CSS, and `:checked` (or `[checked]`) styles the checked state, also when it comes from `{% if done %}checked{% endif %}`. A progress bar or range with `appearance: none` falls back to what Chromium draws then: a grey bar with a green value, a bare thumb.

The parts of a range and a progress bar take CSS through the pseudo-elements pages already use for Chromium and Firefox: `::-webkit-slider-runnable-track` / `::-moz-range-track`, `::-webkit-slider-thumb` / `::-moz-range-thumb`, `::-moz-range-progress` (the track up to the thumb), `::-webkit-progress-bar` and `::-webkit-progress-value` / `::-moz-progress-bar`. A part with CSS is drawn as a box (size, margins, background or gradient, border, radius, shadow; sizes include the border) laid out as Chromium lays it out; parts without CSS keep their native look. Chromium only honors part styles under `appearance: none`; Xerune applies them either way, so `::-webkit-slider-thumb { background: red; }` alone gives a red native-shaped thumb, while the usual web recipe works as in a browser:

```css
input[type=range] { -webkit-appearance: none; width: 200px; }
input[type=range]::-webkit-slider-runnable-track { height: 6px; border-radius: 3px; background: #334155; }
input[type=range]::-webkit-slider-thumb {
    -webkit-appearance: none; width: 18px; height: 18px; border-radius: 50%;
    background: #f59e0b; margin-top: -6px;   /* centers it on the 6px track */
}
progress { appearance: none; height: 10px; }
progress::-webkit-progress-bar { background: #e2e8f0; border-radius: 5px; }
progress::-webkit-progress-value { background: linear-gradient(90deg, #22d3ee, #a78bfa); border-radius: 5px; }
```

A text input shows its `placeholder` while its value is empty, in Chromium's #757575 unless `::placeholder` (or `::-webkit-input-placeholder`) sets `color`, `font-size` or `font-weight`. Part selectors with pseudo-classes (`::-webkit-slider-thumb:hover`) and other pseudo-elements (`::first-line`, `::marker`) are ignored.

**Generated content.** `::before` and `::after` with a `content` string (`""` for an empty box) add a box as the element's first or last child, inheriting from the element and styled by the rule like any `<span>`; `content: none`, `normal` or a non-string value (`attr()`, counters, `url()`) adds nothing. They work on elements whose content is their children and on checkboxes (like in Chromium), not on images, canvases, text inputs, ranges or progress bars. A toggle switch is a checkbox with its knob as `::before`:

```css
.switch { appearance: none; position: relative; width: 40px; height: 22px; margin: 0; border-radius: 11px; background: #1e293b; }
.switch:checked { background: #0ea5e9; }
.switch::before { content: ""; position: absolute; left: 3px; top: 3px; width: 16px; height: 16px; border-radius: 8px; background: #e2e8f0; }
.switch:checked::before { left: 21px; }
```

```html
<input type="checkbox" class="switch" {% if wifi %}checked{% endif %}>
```

Unsupported **tags** fall back to a generic inline-block box (with a one-time warning). Elements with dedicated behaviour: `div`, `p`, `pre`, `span`, `h1`–`h6`, `ul`/`li`, `table`/`tr`/`td`/`th` (+ `thead`/`tbody`/`tfoot`), `strong`/`b`, `img`, `button`, `input` (`type="checkbox"`, `"range"`, text), `progress`, `canvas`, `br`.

---

## 4. Responsive Design

A single template adapts to any screen size using standard CSS building blocks, plus a small Rust-side API. Every example targets an 800x480 panel and switches to a portrait (480x800) layout with an `orientation` media query. Several also use `on_resize`: `showcase` picks a canvas size, `music_player` slides its views by the viewport width, and `breakout` and `animation` lay out their playfield from it. Resize the window, or render headlessly with `XERUNE_SNAPSHOT=out.png XERUNE_SNAPSHOT_SIZE=480x800`, to see the switch.

### Media Queries

`@media` blocks are resolved against the current viewport every time the UI tree is built. Supported features: `min-width`, `max-width`, `min-height`, `max-height` (and `device-*` aliases), `orientation: portrait|landscape`. Media types `all`/`screen` pass, other types (`print`) never match, `not` negates. As in CSS, a comma-separated part with an unknown feature or bad syntax never matches while the other parts still apply, and an empty query matches. Templates and runtime stylesheets read queries the same way.

```css
.card { width: 46%; }
@media (min-width: 1024px) { .card { width: 29%; } }
@media (orientation: landscape) { .toolbar { height: 48px; } }
```

For compiled templates the queries are parsed **at macro time**: each rule inside an `@media` block is inlined into the generated build code behind a `xerune::css::media::matches_query(&[...])` predicate built from static feature constants — matching costs two atomic reads plus a tiny slice scan, no runtime CSS parsing. Runtime stylesheets are parsed once and cached; each build only evaluates their `@media` blocks against the viewport.

### Viewport Units

`vw`, `vh`, `vmin` and `vmax` work anywhere a length is accepted (sizes, padding, gaps, `font-size`, ...):

```css
.hero { width: 80vw; height: 25vh; font-size: 3vh; }
```

### Resize Notification

Backends report viewport changes automatically. On a real size change the runtime updates the global viewport, calls `Model::on_resize`, and rebuilds the tree so media queries and `vw`/`vh` values re-resolve. `on_resize` also fires once with the initial size:

```rust
impl Model for MyModel {
    fn on_resize(&mut self, width: f32, height: f32, _ctx: &mut Context<Self::Message>) {
        self.width = width as u32;
        self.height = height as u32;
    }
}
```

Overriding `on_resize` is only needed when the *structure* of the view changes; pure-CSS adaptation via `@media`/`vw` requires no model code at all.

### Breakpoint Helpers

- `xerune::screen::width()` / `height()` / `size()` — current viewport, readable anywhere (including template expressions like `{{ xerune::screen::width() }}`).
- `xerune::screen::breakpoint()` → `Breakpoint::Mobile` (`<600px`), `Tablet` (`<1024px`), `Desktop`, plus `is_mobile()` / `is_tablet()` / `is_desktop()`. Thresholds are plain constants (`screen::MOBILE_MAX`, `screen::TABLET_MAX`).
- `Context::screen_size()` / `Context::breakpoint()` for use inside `update`/`on_resize`.

---

## 5. Skipping Redundant Rebuilds (view fingerprint)

The classic MVU loop rebuilds the whole UI tree after every message. For embedded CPU savings a model may opt into **fingerprint gating** by overriding `Model::view_fingerprint`:

```rust
fn view_fingerprint(&self) -> Option<u64> {
    // hash everything the view actually renders (e.g. via `xerune::model::hash_bytes`)
    Some(xerune::model::hash_bytes(self.rendered_state.as_bytes()))
}
```

When two consecutive message-driven syncs see an unchanged fingerprint (and no viewport change happened), the runtime skips template re-instantiation, style resolution and text measurement — a big win for messages that only touch state the view does not display (bookkeeping, hidden timers). Canvas pixels remain safe: dirty canvas flags still force a redraw even when the rebuild is skipped, and viewport changes always rebuild.

The default implementation returns `None` (no fingerprinting) and keeps the always-rebuild behavior. **The runtime trusts your fingerprint completely** — if it misses state the view reads, the UI will go stale. Keep it a function of exactly the view-relevant fields.

---

## 6. Keyframe Animations and Transitions

Animations are declared directly in CSS style blocks:

```html
<style>
    @keyframes pulse {
        0% { background-color: #ff0000; }
        50% { background-color: #00ff00; }
        100% { background-color: #ff0000; }
    }
    .pulsing-box {
        width: 100px;
        height: 100px;
        animation-name: pulse;
        animation-duration: 2s;
        animation-iteration-count: infinite;
    }
</style>
<div class="pulsing-box"></div>
```

Animatable properties: `color`, `background-color`, `border-color`, `border-width`, `border-radius`, `font-size`, `width`, `height`, `margin-*`, `padding-*`, `top`/`right`/`bottom`/`left`, and `background` gradients. Two gradients of the same kind with the same number of stops interpolate their angle, center, colors and stop positions, so a spinner is just:

```css
@keyframes spin {
    from { background: conic-gradient(from 0deg, transparent, #22d3ee); }
    to { background: conic-gradient(from 360deg, transparent, #22d3ee); }
}
```

See `examples/animation_css` for a gallery.

### Transitions

`transition` (and `transition-property`, `-duration`, `-delay`,
`-timing-function`, `-behavior`) works as in CSS Transitions 1, on the same
properties (`margin`, `padding`, `inset` and `border` stand for their
longhands; `all` for all of them):

```css
.knob { position: absolute; left: 3px; transition: left 0.18s ease-out; }
.on .knob { left: 21px; }
.btn { background-color: #141c2e; transition: background-color 0.15s; }
.btn:hover { background-color: #1c2740; }
```

When a rebuild (a message, `:hover`, `:active`) changes such a property, it
moves from the value on screen to the new one. A transition sent back partway
returns in the time it took to get there. Elements that appear or disappear
don't transition, and values that can't be interpolated (a color and a
gradient, `auto` and a length) change at once, or halfway with
`transition-behavior: allow-discrete`. Transitions run on frames without
rebuilding the view: paint-only properties just repaint, layout properties
re-run layout. As with keyframes, the browser studio leaves them to the
browser.

An element that appears starts from its `@starting-style` style, if any
(also when the app starts), so it can transition in:

```css
.panel { left: 0%; transition: left 0.3s ease-out; }
@starting-style {
    .panel { left: 100%; }
}
```

Elements send the model messages as their transitions go, like the DOM's
transition events (once per property; they don't bubble):
`data-on-transitionrun` (created, maybe still in its delay),
`data-on-transitionstart` (moving), `data-on-transitionend` and
`data-on-transitioncancel` (reversed, replaced or no longer transitioning). An
element that has slid out of view can leave the template on
`transitionend` instead of being laid out off screen; see the music player's
list and player views. `@starting-style` applies to compiled templates; HTML
parsed at runtime ignores it.

---

## 7. Timers, Intervals and Frames

Models never read a clock: time reaches them as their own messages, through
`update`.

- **One-shot timers** come from `Context::after`, which returns a `TimerId` for
  `Context::cancel`:

  ```rust
  Msg::Saved => {
      self.toast = true;
      self.toast_timer = Some(ctx.after(Duration::from_secs(3), Msg::HideToast));
  }
  ```

- **Intervals and frames** are subscriptions. The runtime asks for them again
  after every `update`, so they follow the model's state and nothing has to be
  started or stopped:

  ```rust
  fn subscriptions(&self) -> Subscriptions<Msg> {
      let subscriptions = Subscriptions::none().every(Duration::from_secs(1), Msg::Clock);
      // Msg::Frame(f32): seconds since the previous frame.
      if self.playing { subscriptions.frames(Msg::Frame) } else { subscriptions }
  }
  ```

  An `every` keeps its phase while the model keeps asking for it (the n-th
  `every` of an interval is the same one).

**Frame rate.** Frames come at the target frame rate (60 by default) while
anything animates: CSS animations and transitions, or a frame subscription.
`Runtime::set_target_fps` / `App::target_fps` set it, like raylib's
`SetTargetFPS`, and `XERUNE_TARGET_FPS` overrides it, to dial the UI down on a
board where other work matters more. Animations take the same time in fewer
steps. A frame that comes too late is dropped, not caught up on. When nothing
animates and no timer is due, the backend sleeps until input arrives.

**Backends and hosts** drive the runtime with `Runtime::poll()`. It handles
everything that is due (loaded images, timers, intervals, the frame message
and the animations' next step) with one rebuild, and returns `Poll { redraw,
wake }`. `wake` says when to call it again: `Wake::Frame(t)` while something
animates, `Wake::At(t)` for the next timer, or `Wake::Idle`. While frames run,
a timer due between two frames is handled with the next one. The runtime reads
the time from its `Clock`: the system clock by default; `no_std` hosts install
their own (`runtime.set_clock(|| Instant::from_millis(board_millis()))`), and
tests use a `ManualClock` they move themselves.

---

## 8. On-screen Keyboard

Devices without a keyboard type text with the runtime's on-screen keyboard.
A model asks for text from `update`; the keyboard then covers the screen, in
place of the model's view, until the user presses OK or Cancel, and the model
gets the result as its own message:

```rust
Msg::EditPassword => ctx.edit_text(
    TextRequest::new("Wi-Fi password").password().min_len(8).max_len(63),
    |password| Msg::Password(password), // Option<String>: None on Cancel
),
```

`TextRequest` sets the title, the text to start from (`.text(..)`), a hidden
password with a Show key (`.password()`), and limits: OK waits for `min_len`
characters, and typing stops at `max_len`. The keyboard has QWERTY letters with
Shift (once, or locked when pressed twice) and two pages of symbols, which
together type all printable ASCII, a single line with a caret moved by the
`‹` `›` keys, and Del. Long text scrolls to keep the caret in view.

While it is open, input goes to the keyboard and not the model: taps press its
keys, text from a hardware keyboard types into it, and other keys act by the
runtime's `KeyMap`. The direction keys move a focus over the keys (up and down
keep the column), Next/Prev (Tab, a remote's or the media keys' next and
previous) go through them in reading order, and Activate (Enter) presses the
focused key, or OK while nothing is focused yet. Escape cancels and Backspace
deletes. The default map knows the names of winit and the evdev backend; a
device with other buttons binds its own:

```rust
runtime.set_key_map(KeyMap::default().bind("KEY_F1", KeyAction::Cancel));
```

The model keeps running underneath (timers, subscriptions, background
messages), and its view, with its scroll positions, comes back when the
keyboard closes. Snapshots reach the keyboard's states with its messages,
which start with `kb:`: `XERUNE_SNAPSHOT_MESSAGES="edit_password;kb:type:a;kb:shift;kb:symbols"`.

---

## 9. Headless Snapshots

Any app can render a single frame to a PNG instead of opening a window, which makes the UI easy to check from tests, CI or an AI agent. Every built-in backend honours these variables, so no code changes are needed:

```bash
XERUNE_SNAPSHOT=todo.png cargo run --example todo
# Reach a specific state, at a specific size, after 1.5 s of simulated time:
XERUNE_SNAPSHOT=todo.png \
XERUNE_SNAPSHOT_SIZE=800x480 \
XERUNE_SNAPSHOT_MESSAGES="remove:0;toggle:0" \
XERUNE_SNAPSHOT_TIME_MS=1500 \
cargo run --example todo
```

| Variable | Meaning |
|---|---|
| `XERUNE_SNAPSHOT` | Output PNG path; enables snapshot mode |
| `XERUNE_SNAPSHOT_SIZE` | `WIDTHxHEIGHT`; defaults to the app's window size |
| `XERUNE_SNAPSHOT_MESSAGES` | `;`-separated messages dispatched in order (same strings as `data-on-click`); an unknown message is an error |
| `XERUNE_SNAPSHOT_POINTER` | `X,Y` where the pointer hovers after the messages, to see `:hover` and `data-on-hover` |
| `XERUNE_SNAPSHOT_TIME_MS` | Simulated time to run before rendering, so timers fire and animations progress (default 0) |

The frame goes through the app's own renderer, so fonts and drawing match the real app. Pending images are awaited (up to 10 s). `App::on_start` is not run, since background producers would make the output nondeterministic; drive state with messages instead. To snapshot from code, use `xerune::backend::SnapshotBackend` directly.

---

## 10. Running on Embedded Linux (fbdev / DRM)

Configure `Cargo.toml` features to enable embedded backends:
```toml
[dependencies]
xerune = { version = "0.1", features = ["linuxfb", "drm", "evdev"] }
```

`App::run` (see section 1) uses DRM/KMS or the fbdev framebuffer
`/dev/fb0` outside a desktop session; `XERUNE_BACKEND=drm` or `=fbdev`
forces one. To run a specific backend directly, `App::run_on(BackendKind::Drm, renderer)`,
or `DrmBackend::new().run(...)` / `LinuxFbBackend::new().run(...)`.

The display's size comes from its mode. For a display mounted rotated,
export `XERUNE_ROTATION=90` (or 180, 270); fbdev also rotates a portrait
framebuffer by itself. Touch input is mapped to the rotated UI.

---

## 11. WebAssembly & Browser Deployment

Xerune applications can compile to WebAssembly to run directly in web browsers or inside **Xerune Studio** (a web-based live inspector).

### 1. Build WASM Bindings
```bash
cd studio
wasm-pack build --target web
```

### 2. Run Local Web Server
```bash
# From workspace root
python3 -m http.server 8000
```

### 3. Open in Browser
- **Studio with live state inspector**: `http://localhost:8000/studio/index.html`
- **Standalone full-screen web runner**: `http://localhost:8000/examples/standalone.html?app=music_player`

### 4. Live templates

In the browser the model runs in wasm, but the view is not compiled in: the
page reads the template files from the server, and the wasm interprets
them (`xerune_template`, the same parser and rules as the derive). Save a
template and the page shows the change within a second, markup and CSS
alike, with no rebuild and no reload; a template error is shown over the
last good view, with its `file:line`. Every element carries a key in
`data-xk`, so updates patch the page in place and transitions, `:hover`
and scroll positions survive. `tests/interpret_tests.rs` checks the
interpreter against the compiled templates.

The template reads the model through serde, so the model must derive
`Serialize`; fields marked `#[serde(skip)]` don't exist for it. Methods
can't be found at run time: the derive builds a table of the methods the
template calls (`self.total()`, `current_track.glow()`), so a template
edit that calls a method it didn't call before needs one rebuild of the
wasm. Their arguments and results go through serde too. A method of a
loop variable (`{% for t in tracks %}{{ t.glow() }}`) can't be called in
the browser; call a method of the model with the index instead. `{{ }}`
shows strings, numbers, `bool`s and `char`s, printed as Rust prints them;
a value of your own type needs a field or method that gives one of those.

To run your own app, export a class with `xerune::export_browser_app!(MyApp, MyModel)`
(feature `browser`) and hand it to `initXeruneApp` from `xerune.js` with
the template's URL (see `studio/src/lib.rs` and `examples/standalone.html`).
Clicks, hover (`data-on-hover`/`-out`) and keys go to `update` as text.
Transition messages come from the runtime, which follows the transitions
of the compiled template.

---

## 12. WPT Reftests

`wpt_runner` checks Xerune against the [web-platform-tests](https://github.com/web-platform-tests/wpt) reftests of the CSS features it implements. Each test is rendered by Xerune and compared pixel for pixel with Chromium's screenshot of the test's reference.

```bash
WPT_DIR=~/path/to/wpt cargo run --release -p wpt_runner             # all suites
WPT_DIR=~/path/to/wpt cargo run --release -p wpt_runner -- flexbox  # paths containing "flexbox"
WPT_DIR=~/path/to/wpt cargo run --release -p wpt_runner -- flexbox --images
WPT_DIR=~/path/to/wpt cargo run --release -p wpt_runner -- --update # record new results
```

Needs Node 22+ and `chromium` (or `$CHROMIUM`). How it works:

- **Scope**: `wpt_runner/suites.txt` lists the WPT directories that count. Grid, transforms, tables, floats, multi-column, writing modes and similar are left out.
- **Comparison**: images must be identical, unless the test's `<meta name=fuzzy>` allows a difference. At least one `match` reference must match and every `mismatch` reference must differ, as in WPT. The one exception is anti-aliasing, because two rasterizers never agree on it exactly (Chromium hints Ahem's glyph boxes onto the pixel grid, fontdue doesn't): a render also matches if every differing pixel is off by at most 64/255 and lies on an edge of the reference. Missing, moved or recolored content differs by far more. The summary's `(aa)` column counts these passes; `--images` saves them for inspection.
- **References**: rendered by Chromium, not Xerune, so a test can't pass by being wrong the same way as its reference. `wpt_runner/chromium_screenshots.mjs` serves `WPT_DIR` over HTTP and screenshots each file at 800x600. Screenshots are cached under `target/wpt/chromium/<wpt commit>-<chromium version>/` and taken again when either changes.
- **Font**: Ahem only, in both Xerune and Chromium (a private fontconfig maps every family to it). Every Ahem glyph is a box, so text compares exactly. Tests that then no longer match their own reference in Chromium are skipped ("fails in Chromium").
- **Skipped**: tests that need JavaScript, `reftest-wait`, iframes, SVG, canvas, video, animations or paging. They are reported but not part of the pass rate.
- **Expectations**: `wpt_runner/expectations.txt` lists the tests that fail; every other test that runs is expected to pass, and its header has the pass rate per suite. A run exits with an error when a test fails that isn't listed, and lists tests that newly pass. After an engine fix, run with `--update` and commit the file along with the fix.
- **Images**: `--images` writes Xerune's render, Chromium's reference and a diff for each failure to `target/wpt/images/failures/<test>/`, and for each anti-aliasing pass to `target/wpt/images/antialiasing/<test>/`.
