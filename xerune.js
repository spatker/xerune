function saveFocus() {
    const active = document.activeElement;
    if (active && (active.tagName === 'INPUT' || active.tagName === 'TEXTAREA')) {
        return {
            id: active.id,
            selectionStart: active.selectionStart,
            selectionEnd: active.selectionEnd
        };
    }
    return null;
}

function restoreFocus(saved) {
    if (!saved) return;
    const el = document.getElementById(saved.id);
    if (el) {
        el.focus();
        try {
            el.setSelectionRange(saved.selectionStart, saved.selectionEnd);
        } catch (e) {}
    }
}

function saveScrollPositions(container) {
    const scrollable = [];
    const elements = container.querySelectorAll('.track-list, .todo-list');
    elements.forEach((el) => {
        if (el.scrollTop > 0 || el.scrollLeft > 0) {
            scrollable.push({
                class: el.classList.contains('track-list') ? 'track-list' : 'todo-list',
                scrollTop: el.scrollTop,
                scrollLeft: el.scrollLeft
            });
        }
    });
    return scrollable;
}

function restoreScrollPositions(container, saved) {
    if (!saved) return;
    saved.forEach(item => {
        const el = container.querySelector('.' + item.class);
        if (el) {
            el.scrollTop = item.scrollTop;
            el.scrollLeft = item.scrollLeft;
        }
    });
}

/**
 * Initializes a Xerune WASM application in the browser DOM.
 * 
 * @param {Object} config
 * @param {Object} config.app Instantiated WASM application (e.g. new TodoApp(""))
 * @param {string} config.containerId DOM element ID where the app should be rendered
 * @param {string} config.templateText Raw HTML template string
 * @param {Function} [config.onStateUpdate] Callback triggered when the state updates
 * @param {Function} [config.onMessageSent] Callback triggered when an event message is dispatched
 */
export function initXeruneApp({ app, containerId, templateText, templateBaseDir, onStateUpdate, onMessageSent }) {
    const container = document.getElementById(containerId);
    if (!container) {
        console.error("Container element not found:", containerId);
        return;
    }

    // 1. Extract style block
    let cleanedTemplateText = templateText;
    const styleMatch = templateText.match(/<style>([\s\S]*?)<\/style>/);
    
    // Remove existing xerune app style element to prevent style pollution
    const oldStyle = document.getElementById("xerune-app-style");
    if (oldStyle) {
        oldStyle.remove();
    }
    
    if (styleMatch) {
        const styleEl = document.createElement("style");
        styleEl.id = "xerune-app-style";
        
        // Scope CSS rules to `#app` to prevent bleeding into the outer studio UI
        const rawCss = styleMatch[1].replace(/\/\*[\s\S]*?\*\//g, "");
        const scopedCss = rawCss.replace(/([^\r\n,{}]+)(,[^\r\n,{}]+)*\s*{(?:[^{}]*|{[^{}]*})*}/g, (match) => {
            const braceIdx = match.indexOf('{');
            if (braceIdx === -1) return match;
            const selectorPart = match.substring(0, braceIdx);
            const bodyPart = match.substring(braceIdx);
            
            const scopedSelectors = selectorPart.split(',').map(sel => {
                sel = sel.trim();
                if (sel === 'body' || sel === 'html') {
                    return '#app';
                }
                if (sel.startsWith('@') || sel.startsWith('to') || sel.startsWith('from') || /^\d+%/.test(sel)) {
                    return sel;
                }
                return `#app ${sel}`;
            });
            
            return scopedSelectors.join(', ') + ' ' + bodyPart;
        });
        
        styleEl.innerHTML = scopedCss;
        document.head.appendChild(styleEl);
        cleanedTemplateText = templateText.replace(/<style>[\s\S]*?<\/style>/, "");
    }

    // Canvas sync helper
    function syncCanvases() {
        const canvases = container.querySelectorAll("canvas");
        for (const canvas of canvases) {
            const id = canvas.id;
            if (!id) continue;
            const pixels = app.get_canvas_pixels(id);
            if (pixels) {
                const width = app.get_canvas_width(id);
                const height = app.get_canvas_height(id);
                if (width > 0 && height > 0) {
                    canvas.width = width;
                    canvas.height = height;
                    const ctx = canvas.getContext("2d");
                    const imgData = ctx.createImageData(width, height);
                    imgData.data.set(new Uint8ClampedArray(pixels));
                    ctx.putImageData(imgData, 0, 0);
                }
            }
        }
    }

    // Initialize Nunjucks environment and pre-compile the template
    const loaderPath = templateBaseDir || '.';
    window.nunjucksEnv = new nunjucks.Environment(new nunjucks.WebLoader(loaderPath));
    window.nunjucksEnv.addFilter('format', function(val, fmt) {
        const fmtStr = val;
        const num = fmt;
        if (typeof fmtStr === 'string' && fmtStr.includes("{:.0}")) {
            return Math.round(num);
        }
        if (typeof fmtStr === 'string' && fmtStr.includes("{:.1}")) {
            return num.toFixed(1);
        }
        if (typeof fmtStr === 'string' && fmtStr.includes("{:.2}")) {
            return num.toFixed(2);
        }
        return num;
    });
    const env = window.nunjucksEnv;
    const compiledTemplate = nunjucks.compile(cleanedTemplateText, env);

    let lastRenderedHtml = "";

    // 2. Render function
    function render() {
        const stateJson = app.get_state_json();
        const state = JSON.parse(stateJson);
        const rendered = compiledTemplate.render(state);
        
        if (rendered !== lastRenderedHtml) {
            const savedFocus = saveFocus();
            const savedScrolls = saveScrollPositions(container);
            
            container.innerHTML = rendered;
            lastRenderedHtml = rendered;
            
            restoreScrollPositions(container, savedScrolls);
            restoreFocus(savedFocus);
        }
        
        syncCanvases();
        
        if (onStateUpdate) {
            onStateUpdate(state);
        }
    }

    // 3. Attach event listeners
    // Click events (via delegation)
    const clickHandler = (event) => {
        console.log("DOM Clicked target:", event.target);
        const target = event.target.closest("[data-on-click]");
        if (target) {
            const action = target.getAttribute("data-on-click");
            console.log("Matched data-on-click action:", action);
            if (action) {
                const changed = app.update(action);
                console.log("app.update returned changed:", changed);
                if (onMessageSent) {
                    onMessageSent(action);
                }
                if (changed) {
                    render();
                }
            }
        }
    };
    container.addEventListener("mousedown", clickHandler);

    // Keypress for text input characters (to support character-by-character updates)
    const keypressHandler = (event) => {
        const target = event.target;
        if (target && target.tagName === "INPUT" && target.type === "text") {
            const id = target.id;
            const char = event.key;
            if (char && char.length === 1) {
                const action = `${id}:text:${char}`;
                const changed = app.update(action);
                if (onMessageSent) {
                    onMessageSent(action);
                }
                if (changed) {
                    render();
                }
                event.preventDefault(); // Let state control the input value
            }
        }
    };
    container.addEventListener("keypress", keypressHandler);

    // Keydown for text inputs (Backspace, Enter, Arrow keys)
    const keydownHandler = (event) => {
        const target = event.target;
        if (target && target.tagName === "INPUT" && target.type === "text") {
            const keys = ["Backspace", "Enter", "ArrowUp", "ArrowDown"];
            if (keys.includes(event.key)) {
                const action = `keydown:${event.key}`;
                const changed = app.update(action);
                if (onMessageSent) {
                    onMessageSent(action);
                }
                if (changed) {
                    render();
                }
                event.preventDefault();
            }
        }
    };
    container.addEventListener("keydown", keydownHandler);

    // Global Keydown for general keyboard navigation & game controls
    const globalKeydownHandler = (event) => {
        if (document.activeElement && document.activeElement.tagName === "INPUT") {
            return;
        }
        const keys = ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Enter", "Space", "Escape"];
        let keyName = event.key;
        if (keyName === " ") keyName = "Space";
        
        if (keys.includes(keyName)) {
            const action = `keydown:${keyName}`;
            const changed = app.update(action);
            if (onMessageSent) {
                onMessageSent(action);
            }
            if (changed) {
                render();
            }
            event.preventDefault();
        }
    };

    // Global Keyup for game controls (so keys_held gets updated correctly)
    const globalKeyupHandler = (event) => {
        if (document.activeElement && document.activeElement.tagName === "INPUT") {
            return;
        }
        const keys = ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Enter", "Space", "Escape"];
        let keyName = event.key;
        if (keyName === " ") keyName = "Space";
        
        if (keys.includes(keyName)) {
            const action = `keyup:${keyName}`;
            const changed = app.update(action);
            if (onMessageSent) {
                onMessageSent(action);
            }
            if (changed) {
                render();
            }
            event.preventDefault();
        }
    };
    
    document.addEventListener("keydown", globalKeydownHandler);
    document.addEventListener("keyup", globalKeyupHandler);

    // 4. Timer tick loop
    let animationFrameId;
    function tick(now) {
        const changed = app.tick(now);
        if (changed) {
            render();
        }
        animationFrameId = requestAnimationFrame(tick);
    }

    // Initial render
    render();
    animationFrameId = requestAnimationFrame(tick);

    // Return cleanup function to destroy the app when switching
    return () => {
        cancelAnimationFrame(animationFrameId);
        container.removeEventListener("mousedown", clickHandler);
        container.removeEventListener("keypress", keypressHandler);
        container.removeEventListener("keydown", keydownHandler);
        document.removeEventListener("keydown", globalKeydownHandler);
        document.removeEventListener("keyup", globalKeyupHandler);
    };
}
