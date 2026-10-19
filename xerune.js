// Runs a Xerune app (a wasm class from `xerune::export_browser_app!`) in a
// browser document: the studio's iframe, or a standalone page's own document.
//
// The model runs in wasm, as on the device. Its view is the template itself,
// read from disk and interpreted in wasm into HTML (`view_html`); the browser
// lays it out and draws it with the template's CSS, and runs its CSS
// animations and transitions. The template files are checked every second,
// so editing the markup or the CSS shows up without rebuilding the wasm or
// reloading the page (calling a model method the template didn't call before
// needs one rebuild: `wasm-pack build --target web` in `studio/`).
//
// Every element carries a key in `data-xk`: re-renders patch the document by
// key instead of replacing it, so transitions, :hover, focus and scroll
// positions survive. Messages go back to the app as text: the elements'
// `data-on-click`, `data-on-hover`, `data-on-hover-out`, `data-on-input`
// and `data-on-change`, and keys.

/**
 * @param {Object} config
 * @param {Object} config.app The wasm app (e.g. `new TodoApp("")`)
 * @param {string} config.templatePath URL of the template file
 * @param {Document} [config.doc] The document to run in (default: this page's)
 * @param {Function} [config.onStateUpdate] Called with the model state after each render
 * @param {Function} [config.onMessageSent] Called with each message sent to the app
 * @returns {Function} Stops the app and removes its listeners
 */
export function initXeruneApp({ app, templatePath, doc = document, onStateUpdate, onMessageSent }) {
    const win = doc.defaultView;
    const listeners = [];
    const listen = (target, type, handler, options) => {
        target.addEventListener(type, handler, options);
        listeners.push(() => target.removeEventListener(type, handler, options));
    };

    // 1. The page: Roboto and no scrollbars, as on the device, the template's
    // CSS, and a place for errors (outside the body, which the view replaces).
    const head = (id, tag) => {
        let el = doc.getElementById(id);
        if (!el) {
            el = doc.createElement(tag);
            el.id = id;
            doc.head.appendChild(el);
        }
        return el;
    };
    const font = (file) => new URL(`resources/fonts/${file}`, import.meta.url);
    head("xerune-base", "style").textContent = `
        @font-face { font-family: Roboto; font-weight: 400; src: url("${font("Roboto-Regular.ttf")}"); }
        @font-face { font-family: Roboto; font-weight: 700; src: url("${font("Roboto-Bold.ttf")}"); }
        html { font-family: Roboto, sans-serif; }
        * { scrollbar-width: none; }
        #xerune-error { position: fixed; inset: auto 8px 8px 8px; z-index: 2147483647; margin: 0; padding: 10px 12px;
            max-height: 50%; overflow: auto; background: #2a0f12f2; color: #ffb4b4; border: 1px solid #ff6b6b;
            border-radius: 6px; font: 12px/1.4 monospace; white-space: pre-wrap; }`;
    const styleEl = head("xerune-style", "style");
    let errorEl = null;
    const showError = (message) => {
        if (!errorEl) {
            errorEl = doc.createElement("pre");
            errorEl.id = "xerune-error";
            doc.documentElement.appendChild(errorEl);
        }
        errorEl.textContent = message;
    };
    const clearError = () => {
        errorEl?.remove();
        errorEl = null;
    };

    // 2. The template, live from disk.
    const root = new URL(templatePath, location.href);
    let lastFiles = null;
    let templateError = null;
    async function loadTemplate() {
        let json;
        try {
            json = JSON.stringify(await templateFiles(root));
        } catch (e) {
            showError(`reading the template failed: ${e.message}`);
            return;
        }
        if (json === lastFiles) return;
        lastFiles = json;
        try {
            app.set_template(root.pathname, json);
            templateError = null;
            styleEl.textContent = app.template_css();
        } catch (e) {
            templateError = String(e);
        }
        render(true);
    }

    // 3. Rendering: patch the document to the app's view.
    const parser = new DOMParser();
    let lastHtml = null;
    function render(force = false) {
        let html;
        try {
            html = app.view_html();
        } catch (e) {
            showError(templateError ?? String(e));
            return;
        }
        if (templateError) {
            showError(templateError);
        } else {
            clearError();
        }
        if (force || html !== lastHtml) {
            lastHtml = html;
            morph(doc.body, parser.parseFromString(html, "text/html").body);
        }
        syncCanvases();
        if (onStateUpdate) {
            onStateUpdate(JSON.parse(app.get_state_json()));
        }
    }

    function send(message) {
        const changed = app.update(message);
        if (onMessageSent) {
            onMessageSent(message);
        }
        if (changed) {
            render();
        }
    }

    function syncCanvases() {
        for (const canvas of doc.querySelectorAll("canvas[id]")) {
            const pixels = app.get_canvas_pixels(canvas.id);
            const width = app.get_canvas_width(canvas.id);
            const height = app.get_canvas_height(canvas.id);
            if (!pixels || width === 0 || height === 0) continue;
            canvas.width = width;
            canvas.height = height;
            const image = canvas.getContext("2d").createImageData(width, height);
            image.data.set(new Uint8ClampedArray(pixels));
            canvas.getContext("2d").putImageData(image, 0, 0);
        }
    }

    // 4. Size, for the model's `on_resize` and `@media`.
    const resize = () => {
        if (app.set_size(doc.documentElement.clientWidth, doc.documentElement.clientHeight)) {
            render();
        }
    };
    listen(win, "resize", resize);

    // 5. Input: the elements' messages, as text.
    listen(doc, "click", (event) => {
        const el = event.target.closest?.("[data-on-click]");
        if (el) {
            send(el.getAttribute("data-on-click"));
        }
    });
    // `mouseenter`/`mouseleave` reach a capturing listener once per element
    // entered or left: entering a child doesn't leave its parent.
    for (const [type, attribute] of [["mouseenter", "data-on-hover"], ["mouseleave", "data-on-hover-out"]]) {
        listen(doc, type, (event) => {
            const message = event.target.getAttribute?.(attribute);
            if (message) {
                send(message);
            }
        }, true);
    }

    // Range inputs: `data-on-input="seek"` sends `seek:<value>` while the
    // thumb moves, `data-on-change` once it is let go of.
    for (const type of ["input", "change"]) {
        listen(doc, type, (event) => {
            const prefix = event.target.getAttribute?.(`data-on-${type}`);
            if (prefix) {
                send(`${prefix}:${event.target.value}`);
            }
        });
    }

    // Text inputs: the model owns their text.
    listen(doc, "keypress", (event) => {
        const target = event.target;
        if (target.tagName === "INPUT" && target.type === "text" && event.key.length === 1) {
            send(`${target.id}:text:${event.key}`);
            event.preventDefault();
        }
    });
    const keys = ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Enter", "Space", "Escape", "Backspace"];
    const keyName = (event) => (event.key === " " ? "Space" : event.key);
    listen(doc, "keydown", (event) => {
        const inInput = event.target.tagName === "INPUT";
        const name = keyName(event);
        if (keys.includes(name) && (!inInput || ["Backspace", "Enter", "ArrowUp", "ArrowDown"].includes(name))) {
            send(`keydown:${name}`);
            event.preventDefault();
        }
    });
    listen(doc, "keyup", (event) => {
        const name = keyName(event);
        if (event.target.tagName !== "INPUT" && keys.includes(name)) {
            send(`keyup:${name}`);
            event.preventDefault();
        }
    });

    // 6. Timers, subscriptions and frames (transition messages too: the
    // runtime follows the transitions of the compiled template).
    let frameId;
    const frame = () => {
        if (app.poll()) {
            render();
        }
        frameId = win.requestAnimationFrame(frame);
    };

    resize();
    const templateTimer = setInterval(loadTemplate, 1000);
    loadTemplate().then(() => {
        frameId = win.requestAnimationFrame(frame);
    });

    return () => {
        win.cancelAnimationFrame(frameId);
        clearInterval(templateTimer);
        listeners.forEach((remove) => remove());
    };
}

/** The template at `url` and the files it includes, by path, fetched fresh. */
async function templateFiles(url, files = {}) {
    if (url.pathname in files) return files;
    const response = await fetch(url, { cache: "no-store" });
    if (!response.ok) throw new Error(`${url.pathname}: ${response.status}`);
    const text = await response.text();
    files[url.pathname] = text;
    for (const m of text.matchAll(/\{%[-+~]?\s*include\s+["']([^"']+)["']\s*[-+~]?%\}/g)) {
        await templateFiles(new URL(m[1], url), files);
    }
    return files;
}

/** Makes element `from` match `to`: attributes, then children. */
function morph(from, to) {
    for (const attr of [...from.attributes]) {
        if (!to.hasAttribute(attr.name)) from.removeAttribute(attr.name);
    }
    for (const attr of to.attributes) {
        if (from.getAttribute(attr.name) !== attr.value) from.setAttribute(attr.name, attr.value);
    }
    if (from.tagName === "INPUT") {
        // The attributes set an input's initial state; what it shows lives in properties.
        const value = to.getAttribute("value") ?? "";
        if (from.value !== value) from.value = value;
        from.checked = to.hasAttribute("checked");
    }
    morphChildren(from, to);
}

/** Makes the children of `from` match those of `to`, keeping elements of
 * the same key (and tag) wherever they move, and text in place. */
function morphChildren(from, to) {
    const keyed = new Map();
    for (const child of from.children) {
        if (child.dataset.xk) keyed.set(child.dataset.xk, child);
    }
    let cursor = from.firstChild;
    for (const next of [...to.childNodes]) {
        let match = null;
        if (next.nodeType === Node.ELEMENT_NODE) {
            const old = keyed.get(next.getAttribute("data-xk"));
            if (old && old.tagName === next.tagName) {
                match = old;
                keyed.delete(next.getAttribute("data-xk"));
            }
        } else if (cursor && cursor.nodeType === next.nodeType) {
            match = cursor;
        }
        if (!match) {
            from.insertBefore(from.ownerDocument.importNode(next, true), cursor);
            continue;
        }
        if (match === cursor) {
            cursor = cursor.nextSibling;
        } else {
            from.insertBefore(match, cursor);
        }
        if (next.nodeType === Node.ELEMENT_NODE) {
            morph(match, next);
        } else if (match.nodeValue !== next.nodeValue) {
            match.nodeValue = next.nodeValue;
        }
    }
    while (cursor) {
        const following = cursor.nextSibling;
        from.removeChild(cursor);
        cursor = following;
    }
}
