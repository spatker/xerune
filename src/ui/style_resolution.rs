use taffy::prelude::*;
use crate::style::{ContainerStyle, RenderData, BoxSizing, Display, CssJustifyContent, TextAlign, Direction, AlignItems, AlignSelf, AlignContent};
use crate::graphics::TextMeasurer;
use crate::css;
use crate::defaults;
use xerune_css::selector::PseudoElement;
use super::node_map::NodeMap;
use super::metadata::NodeMetadata;
use super::Interaction;
use super::builder::TaffyElementWrapper;
use crate::alloc_prelude::*;

/// Cache key representing style attributes of a node to speed up styling resolution.
#[derive(Hash, PartialEq, Eq, Clone)]
pub struct StyleCacheKey {
    /// HTML tag name.
    pub tag: std::borrow::Cow<'static, str>,
    /// Associated HTML class attribute.
    pub class: Option<std::borrow::Cow<'static, str>>,
    /// Associated HTML ID attribute.
    pub id: Option<std::borrow::Cow<'static, str>>,
    /// Other miscellaneous HTML attributes.
    pub other_attrs: Option<Vec<(String, String)>>,
    /// Matching rule indices, including ancestor, sibling and pseudo-class context.
    pub matched_rules: Vec<usize>,
    /// What the element inherits from its parent.
    pub inherited: crate::defaults::Inherited,
    /// Viewport size (rounded px) the resolved `vw`/`vh` lengths were computed against.
    pub viewport: (u32, u32),
}

const fn parse_usize_opt(opt: Option<&'static str>, default: usize) -> usize {
    match opt {
        Some(s) => {
            let bytes = s.as_bytes();
            let mut val = 0;
            let mut i = 0;
            while i < bytes.len() {
                let b = bytes[i];
                if b >= b'0' && b <= b'9' {
                    val = val * 10 + (b - b'0') as usize;
                } else {
                    return default;
                }
                i += 1;
            }
            val
        }
        None => default,
    }
}

/// Compile-time cache capacity limits for embedded environments.
///
/// These values can be configured at compile time via environment variables:
/// - `XERUNE_MAX_STYLESHEET_CACHE`: Max compiled stylesheets to cache (default: 4)
/// - `XERUNE_MAX_STYLE_RULE_CACHE`: Max resolved element style pairs to cache per stylesheet (default: 64)
pub mod cache_config {
    use super::parse_usize_opt;

    /// Maximum number of compiled stylesheets to keep cached.
    /// Default: 4 (covers multi-screen embedded apps like Main, Settings, Error, Modal).
    /// Set to 0 to disable stylesheet caching.
    pub const MAX_STYLESHEET_CACHE_ENTRIES: usize =
        parse_usize_opt(option_env!("XERUNE_MAX_STYLESHEET_CACHE"), 4);

    /// Maximum number of resolved element style rules to cache per stylesheet.
    /// Default: 64 (~23 KB peak heap per stylesheet).
    /// Set to 0 to disable element style caching.
    pub const MAX_STYLE_RULE_CACHE_ENTRIES: usize =
        parse_usize_opt(option_env!("XERUNE_MAX_STYLE_RULE_CACHE"), 64);
}

pub(crate) struct CachedStyles {
    pub(crate) sheet: xerune_css::sheet::StyleSheet,
    pub(crate) keyframes: HashMap<String, css::KeyframesAnimation>,
    pub(crate) style_cache: std::cell::RefCell<HashMap<StyleCacheKey, (Style, ContainerStyle)>>,
}

impl CachedStyles {
    pub(crate) fn new(css: &str, keyframes: HashMap<String, css::KeyframesAnimation>) -> Self {
        Self {
            sheet: xerune_css::sheet::StyleSheet::parse(css),
            keyframes,
            style_cache: std::cell::RefCell::new(HashMap::with_capacity(cache_config::MAX_STYLE_RULE_CACHE_ENTRIES)),
        }
    }
}

pub(crate) struct BoundedStylesheetCache {
    entries: HashMap<String, std::rc::Rc<CachedStyles>>,
    order: std::collections::VecDeque<String>,
    max_entries: usize,
}

impl BoundedStylesheetCache {
    pub(crate) fn new(max_entries: usize) -> Self {
        Self {
            entries: HashMap::with_capacity(max_entries),
            order: std::collections::VecDeque::with_capacity(max_entries),
            max_entries,
        }
    }

    pub(crate) fn get(&self, key: &str) -> Option<std::rc::Rc<CachedStyles>> {
        if self.max_entries == 0 {
            return None;
        }
        self.entries.get(key).cloned()
    }

    pub(crate) fn insert(&mut self, key: String, val: std::rc::Rc<CachedStyles>) {
        if self.max_entries == 0 {
            return;
        }
        if self.entries.contains_key(&key) {
            self.entries.insert(key, val);
            return;
        }
        if self.entries.len() >= self.max_entries {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
        self.order.push_back(key.clone());
        self.entries.insert(key, val);
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

thread_local! {
    /// Keyed by the `@media`-expanded stylesheet text so entries stay viewport-correct.
    pub(crate) static STYLESHEET_CACHE: std::cell::RefCell<BoundedStylesheetCache> =
        std::cell::RefCell::new(BoundedStylesheetCache::new(cache_config::MAX_STYLESHEET_CACHE_ENTRIES));
    pub(crate) static CACHE_STATS: std::cell::Cell<(usize, usize)> = std::cell::Cell::new((0, 0));
    /// Set while [`without_style_cache`] runs.
    static BYPASS_STYLE_CACHE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Runs `f` with element styles resolved anew, neither read from nor
/// written to the style cache: for `XERUNE_VERIFY`, which checks the cache.
pub(crate) fn without_style_cache<T>(f: impl FnOnce() -> T) -> T {
    BYPASS_STYLE_CACHE.with(|bypass| bypass.set(true));
    let result = f();
    BYPASS_STYLE_CACHE.with(|bypass| bypass.set(false));
    result
}

/// Returns the current number of cached stylesheets in thread-local storage.
pub fn stylesheet_cache_len() -> usize {
    STYLESHEET_CACHE.with(|cache| cache.borrow().len())
}

pub(crate) fn to_border_box(dim: taffy::style::Dimension, add: f32) -> taffy::style::Dimension {
    if dim == taffy::style::Dimension::length(dim.value()) {
        taffy::style::Dimension::length(dim.value() + add)
    } else {
        dim
    }
}

pub(crate) fn to_content_box(dim: taffy::style::Dimension, sub: f32) -> taffy::style::Dimension {
    if dim == taffy::style::Dimension::length(dim.value()) {
        taffy::style::Dimension::length((dim.value() - sub).max(0.0))
    } else {
        dim
    }
}

/// Lays out a `block` or `inline-block` element. Taffy has block layout but no
/// inline formatting context, so an element with inline-level children becomes
/// a wrapping flex row that places each child like an atomic inline box; its
/// block-level children take the full width so they start a line of their own.
/// Everything else is a CSS block box (margin collapsing, auto widths).
pub(crate) fn apply_block_display(
    taffy: &mut TaffyTree,
    tag: &str,
    style: &ContainerStyle,
    layout_style: &mut Style,
    node: NodeId,
    render_data: &NodeMap<RenderData>,
) {
    let is_inline = |child: NodeId| match render_data.get(child) {
        Some(data) => data.container_style().is_none_or(|s| s.display.is_inline_level() || s.is_floated),
        None => false,
    };
    if !taffy.child_ids(node).any(is_inline) && tag != "tr" {
        layout_style.display = taffy::style::Display::Block;
        return;
    }
    layout_style.display = taffy::style::Display::Flex;
    layout_style.flex_direction = FlexDirection::Row;
    layout_style.flex_wrap = FlexWrap::Wrap;
    if let Some(align) = style.text_align {
        layout_style.justify_content = Some(match align {
            TextAlign::Right => JustifyContent::FlexEnd,
            TextAlign::Center => JustifyContent::Center,
            TextAlign::Left => JustifyContent::FlexStart,
        });
    }
    for child in taffy.children(node).unwrap_or_default() {
        if render_data.get(child).and_then(RenderData::container_style).is_some_and(|s| s.display == Display::Block) {
            if let Ok(mut child_style) = taffy.style(child).cloned() {
                if child_style.size.width.is_auto() {
                    child_style.size.width = Dimension::percent(1.0);
                    let _ = taffy.set_style(child, child_style);
                }
            }
        }
    }
}

/// Where runtime styling puts the `@starting-style` styles of new
/// elements, for a stylesheet that has any.
pub(crate) struct Starting<'a> {
    /// The keys of the view before (see [`super::builder::is_new`]).
    pub previous_keys: Option<&'a std::collections::HashSet<u64>>,
    pub styles: &'a mut NodeMap<(Style, ContainerStyle)>,
}

/// The attributes of an element built at runtime, `style` included.
fn element_attrs(meta: &NodeMetadata) -> Vec<(&str, &str)> {
    match &meta.other_attrs {
        Some(other) => meta.style.as_deref().map(|style| ("style", style)).into_iter().chain(other.iter().map(|(k, v)| (k.as_str(), v.as_str()))).collect(),
        None => meta.attrs.iter().filter(|(k, _)| k != "class").map(|(k, v)| (k.as_str(), v.as_str())).collect(),
    }
}

/// The style a new element starts from by `@starting-style`: its matched
/// `rules`, the starting ones among them, applied as a compiled template
/// applies them (see `xerune_derive`'s starting styles), with the same
/// [`super::builder::ElementStyle`] steps.
fn starting_style(
    meta: &NodeMetadata,
    defaults_key: &str,
    parent_style: &ContainerStyle,
    stylesheet: &xerune_css::sheet::StyleSheet,
    rules: &[usize],
    message_validator: &dyn Fn(&str) -> bool,
) -> (Style, ContainerStyle) {
    let mut el = super::builder::ElementStyle::with_defaults(meta.tag.clone(), defaults_key, parent_style);
    let declarations = |important: bool| rules.iter().map(|&i| &stylesheet.rules[i]).filter(|rule| rule.selector.pseudo_element().is_none())
        .map(move |rule| rule.declarations.iter().filter(|d| d.important == important).map(|d| (d.name.as_str(), d.value.as_str())).collect::<Vec<_>>());
    for declarations in declarations(false) {
        el.apply(&declarations);
    }
    let attrs = element_attrs(meta);
    el.read_attributes(attrs.iter().copied(), message_validator);
    for declarations in declarations(true) {
        el.apply(&declarations);
    }
    if let Some((_, style)) = attrs.iter().find(|(k, _)| *k == "style") {
        el.apply_important_inline(style);
    }
    el.into_box(parent_style)
}

pub(crate) fn resolve_styles(
    taffy: &mut TaffyTree,
    node: NodeId,
    text_measurer: &impl TextMeasurer,
    render_data: &mut NodeMap<RenderData>,
    interactions: &mut NodeMap<Interaction>,
    parent_style: ContainerStyle,
    message_validator: &impl Fn(&str) -> bool,
    stylesheet: &xerune_css::sheet::StyleSheet,
    media: &[bool],
    node_metadata: &NodeMap<NodeMetadata>,
    keys: &mut NodeMap<u64>,
    pointer: &super::builder::PointerState,
    hover_handlers: &mut NodeMap<super::HoverHandlers>,
    transition_handlers: &mut NodeMap<super::TransitionHandlers>,
    value_handlers: &mut NodeMap<super::ValueHandlers>,
    base_styles: &mut NodeMap<(Style, ContainerStyle)>,
    style_cache: &mut HashMap<StyleCacheKey, (Style, ContainerStyle)>,
    starting: &mut Option<Starting<'_>>,
) {
    let meta = match node_metadata.get(&node) {
        Some(m) => m,
        None => return,
    };

    if meta.tag == "#text" {
        let current_style = super::text_style(&parent_style);

        if let Some(ref text) = meta.text {
            let normalized = super::collapse_white_space(text, current_style.white_space);
            if !normalized.is_empty() {
                // Sized by the layout's measure function (see `Ui::compute_layout`).
                let text_layout_style = super::text_leaf_style(&current_style);
                let _ = taffy.set_style(node, text_layout_style.clone());
                if current_style.animates() {
                    base_styles.insert(node, (text_layout_style, current_style.clone()));
                }
                render_data.insert(node, RenderData::new(crate::style::RenderKind::Text(normalized.into_owned()), current_style));
            }
        }
        return;
    }

    let tag = &meta.tag;
    let input_type = meta.other_attrs.iter().flatten().chain(meta.attrs.iter())
        .find(|(k, _)| k == "type")
        .map(|(_, v)| v.as_str());
    let defaults_key = defaults::element_defaults_key(tag, input_type);
    let element_type = defaults::get_default_style(defaults_key, &ContainerStyle::default()).element_type;
    let mut parsed = super::attributes::ParsedAttributes::new(element_type);

    if let Some(checked) = meta.checked {
        parsed.checkbox_checked = checked;
    }
    if let Some(slider_value) = meta.slider_value {
        parsed.range.value = Some(slider_value);
    }
    if let Some(progress_value) = meta.progress_value {
        parsed.progress_value = progress_value;
    }
    if let Some(progress_max) = meta.progress_max {
        parsed.progress_max = progress_max;
    }
    if let Some(ref image_src) = meta.image_src {
        parsed.image_src = image_src.clone();
    }
    if let Some(ref canvas_id) = meta.canvas_id {
        parsed.canvas_id = canvas_id.clone();
    }
    if let Some(ref input_text) = meta.input_text {
        parsed.text_input_text = Some(input_text.clone());
    }

    let (vw, vh) = crate::screen::size();
    let el_wrapper = TaffyElementWrapper {
        node,
        taffy,
        metadata: node_metadata,
        meta,
        keys: &*keys,
        pointer,
    };
    let matches = |rule: &xerune_css::sheet::Rule| rule.media.is_none_or(|m| media[m]) && rule.selector.matches(&el_wrapper);
    let matched_rules = stylesheet.rules.iter().enumerate()
        .filter_map(|(index, rule)| (!rule.starting && matches(rule)).then_some(index))
        .collect();
    // A new element's rules with the `@starting-style` ones, if it has any.
    let starting_rules = starting.as_ref()
        .filter(|starting| keys.get(node).is_some_and(|&key| super::builder::is_new(starting.previous_keys, key)))
        .map(|_| stylesheet.rules.iter().enumerate().filter_map(|(index, rule)| matches(rule).then_some(index)).collect::<Vec<_>>())
        .filter(|rules| rules.iter().any(|&index| stylesheet.rules[index].starting));
    let cache_key = StyleCacheKey {
        tag: meta.tag.clone(),
        class: meta.class.clone(),
        id: meta.id.clone(),
        other_attrs: meta.other_attrs.clone(),
        matched_rules,
        inherited: crate::defaults::Inherited::of(&parent_style),
        viewport: (vw.round() as u32, vh.round() as u32),
    };

    // Applied after the `style` attribute, so not part of the cached style.
    let important_rules: Vec<usize> = match stylesheet.has_important {
        true => cache_key.matched_rules.iter().copied()
            .filter(|&i| stylesheet.rules[i].selector.pseudo_element().is_none() && stylesheet.rules[i].declarations.iter().any(|d| d.important))
            .collect(),
        false => Vec::new(),
    };
    let bypass = BYPASS_STYLE_CACHE.with(|bypass| bypass.get());
    let cached = if bypass { None } else { style_cache.get(&cache_key) };
    let (mut layout_style, mut current_style) = if let Some(cached_styles) = cached {
        CACHE_STATS.with(|stats| {
            let (hits, misses) = stats.get();
            stats.set((hits + 1, misses));
        });
        cached_styles.clone()
    } else {
        CACHE_STATS.with(|stats| {
            let (hits, misses) = stats.get();
            stats.set((hits, misses + 1));
        });
        let defaults = defaults::get_default_style(defaults_key, &parent_style);
        let mut l_style = defaults.taffy_style;
        let mut c_style = defaults.container_style;

        for font_pass in [true, false] {
            for &index in &cache_key.matched_rules {
                let rule = &stylesheet.rules[index];
                match rule.selector.pseudo_element() {
                    None => {
                        // `!important` ones come after the `style` attribute.
                        for decl in rule.declarations.iter().filter(|d| !d.important && css::sets_font_size(&d.name) == font_pass) {
                            let _ = css::apply_declaration(&decl.name, &decl.value, &mut c_style, &mut l_style);
                        }
                    }
                    // Kept as written: resolved for the part when it is built.
                    // Only form controls have parts (`*::-moz-range-thumb`).
                    Some(PseudoElement::Part(part)) if !font_pass && (!part.of_form_control() || matches!(&**tag, "input" | "progress")) => {
                        c_style.part_declarations.extend(rule.declarations.iter().map(|d| crate::style::PartDeclaration {
                            part,
                            name: d.name.clone(),
                            value: d.value.clone(),
                        }));
                    }
                    Some(_) => {}
                }
            }
        }
        let pair = (l_style, c_style);
        if cache_config::MAX_STYLE_RULE_CACHE_ENTRIES > 0 && !bypass {
            if style_cache.len() >= cache_config::MAX_STYLE_RULE_CACHE_ENTRIES {
                style_cache.clear();
            }
            style_cache.insert(cache_key, pair.clone());
        }
        pair
    };

    // 1. Process id attribute
    let id_str = meta.id.as_deref().or_else(|| {
        if !meta.attrs.is_empty() {
            for (k, v) in &meta.attrs {
                if k == "id" {
                    return Some(v.as_str());
                }
            }
        }
        None
    });
    if let Some(id_str) = id_str {
        parsed.canvas_id = id_str.to_string();
        parsed.element_id = Some(id_str.to_string());
    }

    // 2. Process inline styles
    let style_str = meta.style.as_deref().or_else(|| {
        if !meta.attrs.is_empty() {
            for (k, v) in &meta.attrs {
                if k == "style" {
                    return Some(v.as_str());
                }
            }
        }
        None
    });
    if let Some(style_str) = style_str {
        css::parse_inline_style(style_str, &mut current_style, &mut layout_style);
    }
    for font_pass in [true, false] {
        for &index in &important_rules {
            for decl in stylesheet.rules[index].declarations.iter().filter(|d| d.important && css::sets_font_size(&d.name) == font_pass) {
                let _ = css::apply_declaration(&decl.name, &decl.value, &mut current_style, &mut layout_style);
            }
        }
    }
    if let Some(style_str) = style_str {
        css::parse_inline_style_important(style_str, &mut current_style, &mut layout_style);
    }

    // 3. Process other attributes using generic parser
    if let Some(ref other) = meta.other_attrs {
        super::attributes::parse_attributes_generic(
            tag,
            other.iter().map(|(k, v)| (k.as_str(), v.as_str())),
            &mut current_style,
            &mut layout_style,
            &mut parsed,
            message_validator,
        );
    } else if !meta.attrs.is_empty() {
        super::attributes::parse_attributes_generic(
            tag,
            meta.attrs.iter().filter(|(k, _)| k != "style" && k != "class" && k != "id").map(|(k, v)| (k.as_str(), v.as_str())),
            &mut current_style,
            &mut layout_style,
            &mut parsed,
            message_validator,
        );
    }

    resolve_box(&parent_style, &mut layout_style, &mut current_style);
    if let (Some(rules), Some(starting)) = (starting_rules, starting.as_mut()) {
        starting.styles.insert(node, starting_style(meta, defaults_key, &parent_style, stylesheet, &rules, message_validator));
    }

    let children = &meta.children;
    for &child in children {
        resolve_styles(
            taffy,
            child,
            text_measurer,
            render_data,
            interactions,
            current_style.clone(),
            message_validator,
            stylesheet,
            media,
            node_metadata,
            keys,
            pointer,
            hover_handlers,
            transition_handlers,
            value_handlers,
            base_styles,
            style_cache,
            starting,
        );
    }

    if !parsed.hover.is_empty() {
        hover_handlers.insert(node, parsed.hover.clone());
    }
    if !parsed.transition.is_empty() {
        transition_handlers.insert(node, parsed.transition.clone());
    }
    if !parsed.value.is_empty() {
        value_handlers.insert(node, parsed.value.clone());
    }
    let generated = finish_node(node, tag, &parent_style, layout_style, current_style, &parsed, taffy, render_data, interactions, base_styles);
    if let Some(&key) = keys.get(node) {
        super::builder::key_generated_boxes(keys, key, generated);
    }
}

/// Finalizes a node's styles after processing all attributes and stylesheet overrides.
/// This is used internally and by generated proc-macro template structures.
pub fn finalize_node_style(
    node: NodeId,
    tag: &str,
    parent_style: &ContainerStyle,
    mut layout_style: Style,
    mut current_style: ContainerStyle,
    parsed: &crate::ui::attributes::ParsedAttributes,
    taffy: &mut TaffyTree,
    render_data: &mut NodeMap<RenderData>,
    interactions: &mut NodeMap<Interaction>,
    base_styles: &mut NodeMap<(Style, ContainerStyle)>,
) -> GeneratedBoxes {
    resolve_box(parent_style, &mut layout_style, &mut current_style);
    finish_node(node, tag, parent_style, layout_style, current_style, parsed, taffy, render_data, interactions, base_styles)
}

/// Sizes, `box-sizing` and the static position of an absolutely positioned
/// box: the part of [`finalize_node_style`] its children's style can see.
pub(crate) fn resolve_box(parent_style: &ContainerStyle, layout_style: &mut Style, current_style: &mut ContainerStyle) {
    if let Some(d) = current_style.inline_size {
        layout_style.size.width = d;
        if d == taffy::style::Dimension::length(d.value()) {
            current_style.width = Some(d.value());
        }
    }
    if let Some(d) = current_style.block_size {
        layout_style.size.height = d;
        if d == taffy::style::Dimension::length(d.value()) {
            current_style.height = Some(d.value());
        }
    }
    if let Some(d) = current_style.min_inline_size {
        layout_style.min_size.width = d;
    }
    if let Some(d) = current_style.max_inline_size {
        layout_style.max_size.width = d;
    }
    if let Some(d) = current_style.min_block_size {
        layout_style.min_size.height = d;
    }
    if let Some(d) = current_style.max_block_size {
        layout_style.max_size.height = d;
    }

    let add_h = current_style.padding_left + current_style.padding_right + current_style.border.horizontal_width();
    let add_v = current_style.padding_top + current_style.padding_bottom + current_style.border.vertical_width();

    if current_style.box_sizing == BoxSizing::ContentBox {
        layout_style.size.width = to_border_box(layout_style.size.width, add_h);
        layout_style.size.height = to_border_box(layout_style.size.height, add_v);
    } else {
        layout_style.min_size.width = to_content_box(layout_style.min_size.width, add_h);
        layout_style.min_size.height = to_content_box(layout_style.min_size.height, add_v);
        layout_style.max_size.width = to_content_box(layout_style.max_size.width, add_h);
        layout_style.max_size.height = to_content_box(layout_style.max_size.height, add_v);
    }

    if layout_style.position == Position::Absolute {
        enum Alignment {
            Start,
            End,
            Center,
        }

        if layout_style.inset.left.is_auto() && layout_style.inset.right.is_auto() {
            let is_parent_row = parent_style.flex_direction == FlexDirection::Row || parent_style.flex_direction == FlexDirection::RowReverse;
            let h_align = if is_parent_row {
                let jc = parent_style.justify_content.unwrap_or(CssJustifyContent::FlexStart);
                match jc {
                    CssJustifyContent::FlexStart => {
                        if (parent_style.flex_direction == FlexDirection::Row) ^ (parent_style.direction == Direction::Rtl) {
                            Alignment::Start
                        } else {
                            Alignment::End
                        }
                    }
                    CssJustifyContent::FlexEnd => {
                        if (parent_style.flex_direction == FlexDirection::Row) ^ (parent_style.direction == Direction::Rtl) {
                            Alignment::End
                        } else {
                            Alignment::Start
                        }
                    }
                    CssJustifyContent::Center | CssJustifyContent::SpaceAround | CssJustifyContent::SpaceEvenly => {
                        Alignment::Center
                    }
                    CssJustifyContent::SpaceBetween => {
                        if (parent_style.flex_direction == FlexDirection::Row) ^ (parent_style.direction == Direction::Rtl) {
                            Alignment::Start
                        } else {
                            Alignment::End
                        }
                    }
                    CssJustifyContent::Start => {
                        if parent_style.direction == Direction::Rtl {
                            Alignment::End
                        } else {
                            Alignment::Start
                        }
                    }
                    CssJustifyContent::End => {
                        if parent_style.direction == Direction::Rtl {
                            Alignment::Start
                        } else {
                            Alignment::End
                        }
                    }
                    CssJustifyContent::Left => {
                        Alignment::Start
                    }
                    CssJustifyContent::Right => {
                        Alignment::End
                    }
                }
            } else {
                let align = layout_style.align_self.map(|s| match s {
                    AlignSelf::FlexStart | AlignSelf::Start => AlignItems::FlexStart,
                    AlignSelf::FlexEnd | AlignSelf::End => AlignItems::FlexEnd,
                    AlignSelf::Center => AlignItems::Center,
                    AlignSelf::Baseline => AlignItems::Baseline,
                    AlignSelf::Stretch => AlignItems::Stretch,
                }).unwrap_or_else(|| parent_style.align_items.unwrap_or(AlignItems::Stretch));

                match align {
                    AlignItems::FlexStart | AlignItems::Stretch | AlignItems::Baseline | AlignItems::Start => {
                        if (parent_style.direction == Direction::Rtl) ^ (parent_style.flex_wrap == FlexWrap::WrapReverse) {
                            Alignment::End
                        } else {
                            Alignment::Start
                        }
                    }
                    AlignItems::FlexEnd | AlignItems::End => {
                        if (parent_style.direction == Direction::Rtl) ^ (parent_style.flex_wrap == FlexWrap::WrapReverse) {
                            Alignment::Start
                        } else {
                            Alignment::End
                        }
                    }
                    AlignItems::Center => {
                        Alignment::Center
                    }
                }
            };

            match h_align {
                Alignment::Start => {
                    layout_style.inset.left = length(parent_style.padding_left);
                }
                Alignment::End => {
                    if let (Some(pw), Some(cw)) = (parent_style.width, current_style.width) {
                        layout_style.inset.left = length(pw - cw + parent_style.padding_left);
                    } else {
                        layout_style.inset.right = length(parent_style.padding_right);
                    }
                }
                Alignment::Center => {
                    if let (Some(pw), Some(cw)) = (parent_style.width, current_style.width) {
                        layout_style.inset.left = length((pw - cw) / 2.0 + parent_style.padding_left);
                    } else {
                        layout_style.inset.left = length(parent_style.padding_left);
                    }
                }
            }
        }

        if layout_style.inset.top.is_auto() && layout_style.inset.bottom.is_auto() {
            let is_parent_row = parent_style.flex_direction == FlexDirection::Row || parent_style.flex_direction == FlexDirection::RowReverse;
            let v_align = if !is_parent_row {
                let jc = parent_style.justify_content.unwrap_or(CssJustifyContent::FlexStart);
                match jc {
                    CssJustifyContent::FlexStart => {
                        if parent_style.flex_direction == FlexDirection::Column {
                            Alignment::Start
                        } else {
                            Alignment::End
                        }
                    }
                    CssJustifyContent::FlexEnd => {
                        if parent_style.flex_direction == FlexDirection::Column {
                            Alignment::End
                        } else {
                            Alignment::Start
                        }
                    }
                    CssJustifyContent::Center | CssJustifyContent::SpaceAround | CssJustifyContent::SpaceEvenly => {
                        Alignment::Center
                    }
                    CssJustifyContent::SpaceBetween => {
                        if parent_style.flex_direction == FlexDirection::Column {
                            Alignment::Start
                        } else {
                            Alignment::End
                        }
                    }
                    CssJustifyContent::Start | CssJustifyContent::Left | CssJustifyContent::Right => {
                        Alignment::Start
                    }
                    CssJustifyContent::End => {
                        Alignment::End
                    }
                }
            } else {
                let align = layout_style.align_self.map(|s| match s {
                    AlignSelf::FlexStart | AlignSelf::Start => AlignItems::FlexStart,
                    AlignSelf::FlexEnd | AlignSelf::End => AlignItems::FlexEnd,
                    AlignSelf::Center => AlignItems::Center,
                    AlignSelf::Baseline => AlignItems::Baseline,
                    AlignSelf::Stretch => AlignItems::Stretch,
                }).unwrap_or_else(|| parent_style.align_items.unwrap_or(AlignItems::Stretch));

                match align {
                    AlignItems::FlexStart | AlignItems::Stretch | AlignItems::Baseline | AlignItems::Start => {
                        if parent_style.flex_wrap == FlexWrap::WrapReverse {
                            Alignment::End
                        } else {
                            Alignment::Start
                        }
                    }
                    AlignItems::FlexEnd | AlignItems::End => {
                        if parent_style.flex_wrap == FlexWrap::WrapReverse {
                            Alignment::Start
                        } else {
                            Alignment::End
                        }
                    }
                    AlignItems::Center => {
                        Alignment::Center
                    }
                }
            };

            match v_align {
                Alignment::Start => {
                    layout_style.inset.top = length(parent_style.padding_top);
                }
                Alignment::End => {
                    if let (Some(ph), Some(ch)) = (parent_style.height, current_style.height) {
                        layout_style.inset.top = length(ph - ch + parent_style.padding_top);
                    } else {
                        layout_style.inset.bottom = length(parent_style.padding_bottom);
                    }
                }
                Alignment::Center => {
                    if let (Some(ph), Some(ch)) = (parent_style.height, current_style.height) {
                        layout_style.inset.top = length((ph - ch) / 2.0 + parent_style.padding_top);
                    } else {
                        layout_style.inset.top = length(parent_style.padding_top);
                    }
                }
            }
        }
    }
}

/// The rest of [`finalize_node_style`], once the element's children are
/// built: generated boxes, display, layout fix-ups, and its render data.
pub(crate) fn finish_node(
    node: NodeId,
    tag: &str,
    parent_style: &ContainerStyle,
    mut layout_style: Style,
    mut current_style: ContainerStyle,
    parsed: &crate::ui::attributes::ParsedAttributes,
    taffy: &mut TaffyTree,
    render_data: &mut NodeMap<RenderData>,
    interactions: &mut NodeMap<Interaction>,
    base_styles: &mut NodeMap<(Style, ContainerStyle)>,
) -> GeneratedBoxes {
    // Generated boxes, then flex items in `order` (Taffy has no `order`).
    let generated = super::pseudo::generate(node, parsed.element_type, &mut current_style, taffy, render_data, interactions, base_styles);
    let order = |child: &NodeId| render_data.get(*child).map(|data| data.style.order).unwrap_or(0);
    if current_style.display.is_flex() && taffy.child_ids(node).any(|child| order(&child) != 0) {
        let mut children = taffy.children(node).unwrap_or_default();
        children.sort_by_key(order);
        let _ = taffy.set_children(node, &children);
    }

    if current_style.display == Display::None {
        layout_style.display = taffy::style::Display::None;
    } else if !current_style.display.is_flex() {
        apply_block_display(taffy, tag, &mut current_style, &mut layout_style, node, render_data);
    }

    layout_style.border = current_style.border.widths().map(length);

    if current_style.direction == Direction::Rtl {
        match layout_style.flex_direction {
            FlexDirection::Row | FlexDirection::RowReverse => {
                layout_style.flex_direction = match layout_style.flex_direction {
                    FlexDirection::Row => FlexDirection::RowReverse,
                    FlexDirection::RowReverse => FlexDirection::Row,
                    other => other,
                };
                layout_style.justify_content = match layout_style.justify_content {
                    Some(JustifyContent::FlexStart) | None => Some(JustifyContent::FlexEnd),
                    Some(JustifyContent::FlexEnd) => Some(JustifyContent::FlexStart),
                    other => other,
                };
            }
            FlexDirection::Column | FlexDirection::ColumnReverse => {
                layout_style.align_items = match layout_style.align_items {
                    Some(AlignItems::FlexStart) | None => Some(AlignItems::FlexEnd),
                    Some(AlignItems::FlexEnd) => Some(AlignItems::FlexStart),
                    other => other,
                };
                layout_style.align_content = match layout_style.align_content {
                    Some(AlignContent::FlexStart) | None => Some(AlignContent::FlexEnd),
                    Some(AlignContent::FlexEnd) => Some(AlignContent::FlexStart),
                    other => other,
                };
            }
        }
    }

    if !layout_style.max_size.height.is_auto() {
        let val = layout_style.max_size.height.value();
        if layout_style.max_size.height == Dimension::length(val) {
            layout_style.max_size.height = Dimension::length(val + current_style.border.vertical_width() + current_style.padding_top + current_style.padding_bottom);
        }
    }
    if !layout_style.max_size.width.is_auto() {
        let val = layout_style.max_size.width.value();
        if layout_style.max_size.width == Dimension::length(val) {
            layout_style.max_size.width = Dimension::length(val + current_style.border.horizontal_width() + current_style.padding_left + current_style.padding_right);
        }
    }
    if !layout_style.min_size.height.is_auto() {
        let val = layout_style.min_size.height.value();
        if layout_style.min_size.height == Dimension::length(val) {
            layout_style.min_size.height = Dimension::length(val + current_style.border.vertical_width() + current_style.padding_top + current_style.padding_bottom);
        }
    }
    if !layout_style.min_size.width.is_auto() {
        let val = layout_style.min_size.width.value();
        if layout_style.min_size.width == Dimension::length(val) {
            layout_style.min_size.width = Dimension::length(val + current_style.border.horizontal_width() + current_style.padding_left + current_style.padding_right);
        }
    }

    let is_parent_flex = parent_style.display.is_flex();
    if is_parent_flex {
        let resolved_align = match layout_style.align_self {
            None => parent_style.align_items.unwrap_or(AlignItems::Stretch),
            Some(AlignSelf::FlexStart) => AlignItems::FlexStart,
            Some(AlignSelf::FlexEnd) => AlignItems::FlexEnd,
            Some(AlignSelf::Center) => AlignItems::Center,
            Some(AlignSelf::Baseline) => AlignItems::Baseline,
            Some(AlignSelf::Stretch) => AlignItems::Stretch,
            Some(AlignSelf::Start) => AlignItems::Start,
            Some(AlignSelf::End) => AlignItems::End,
        };
        if resolved_align == AlignItems::Baseline {
            let is_column = parent_style.flex_direction == FlexDirection::Column 
                || parent_style.flex_direction == FlexDirection::ColumnReverse;
            if is_column {
                layout_style.align_self = Some(AlignSelf::FlexStart);
            }
        }
    }

    if let Some(interaction) = parsed.interaction_id.clone() {
        interactions.insert(node, interaction);
    }

    if current_style.animates() {
        base_styles.insert(node, (layout_style.clone(), current_style.clone()));
    }
    let _ = taffy.set_style(node, layout_style);
    crate::ui::attributes::process_element_type(node, parsed, current_style, render_data);
    generated
}

/// The `::before` and `::after` boxes `finish_node` generated for an
/// element, which get keys from the element's.
pub type GeneratedBoxes = (Option<NodeId>, Option<NodeId>);
