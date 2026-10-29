pub(crate) mod node_map;
pub(crate) mod metadata;
pub(crate) mod builder;
mod controls;
mod pseudo;
#[cfg(feature = "dynamic-parser")]
pub mod html;
pub mod attributes;
/// Module containing stylesheet layout mapping resolver methods.
pub mod style_resolution;

pub use node_map::{NodeMap, NodeMapIter, NodeMapValues};
pub use metadata::NodeMetadata;
pub use builder::{ElementStyle, PointerLevel, PointerState, UiBuilder};
pub use style_resolution::{cache_config, stylesheet_cache_len};

use taffy::prelude::*;
use taffy::TaffyError;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

#[cfg(feature = "dynamic-parser")]
use html5ever::parse_document;
#[cfg(feature = "dynamic-parser")]
use html5ever::tendril::TendrilSink;
#[cfg(feature = "dynamic-parser")]
use markup5ever_rcdom::{Handle as DomHandle, RcDom};

/// Representation handle to an HTML DOM node.
#[cfg(feature = "dynamic-parser")]
pub type Handle = DomHandle;

/// Representation handle to an HTML DOM node (unused when dynamic-parser feature is disabled).
#[cfg(not(feature = "dynamic-parser"))]
pub type Handle = ();

use crate::graphics::{Canvas, DrawCommand, Rect, TextMeasurer};
use crate::style::{ContainerStyle, Overflow, RenderData, RenderKind};
use crate::css;

/// What clicking an element sends to the model.
#[derive(Clone)]
pub enum Interaction {
    /// A message value built by a compiled template; the runtime downcasts
    /// it to the model's message type.
    Message(Rc<dyn core::any::Any>),
    /// A message string, parsed with `FromStr` when clicked (hand-written
    /// and runtime-parsed templates).
    Text(String),
}

impl core::fmt::Debug for Interaction {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Interaction::Message(_) => f.write_str("Message(..)"),
            Interaction::Text(text) => f.debug_tuple("Text").field(text).finish(),
        }
    }
}

/// A view compiled from an HTML template, implemented by
/// `#[derive(XeruneTemplate)]`. Its methods are internal and change with the
/// derive macro.
pub trait TemplateLayout {
    /// The CSS stylesheet content accompanying this template.
    #[doc(hidden)]
    fn stylesheet(&self) -> &'static str;
    /// Builds the layout tree onto the builder, returning the root NodeId.
    #[doc(hidden)]
    fn build_ui(
        &self,
        builder: &mut UiBuilder,
        measurer: &impl TextMeasurer,
        default_style: &ContainerStyle,
        message_validator: &impl Fn(&str) -> bool,
    ) -> NodeId;
    /// Whether the template's rules use `:hover` and `:active`.
    #[doc(hidden)]
    fn uses_pointer(&self) -> (bool, bool) {
        (false, false)
    }
}

/// The messages an element sends when the pointer enters it
/// (`data-on-hover`) and leaves it (`data-on-hover-out`). Entering a child
/// doesn't leave its parent: the parent is still hovered.
#[derive(Debug, Clone, Default)]
pub struct HoverHandlers {
    /// Sent when the element becomes hovered.
    pub enter: Option<Interaction>,
    /// Sent when it stops being hovered.
    pub leave: Option<Interaction>,
}

impl HoverHandlers {
    fn is_empty(&self) -> bool {
        self.enter.is_none() && self.leave.is_none()
    }
}

/// The messages an element sends as its transitions go
/// (`data-on-transitionrun`, `-transitionstart`, `-transitionend`,
/// `-transitioncancel`, after the DOM's transition events). Each is sent
/// once per transitioning property; unlike DOM events they don't bubble.
#[derive(Debug, Clone, Default)]
pub struct TransitionHandlers {
    /// A transition was created (it may still wait out its delay).
    pub run: Option<Interaction>,
    /// Its delay is over: it starts moving.
    pub start: Option<Interaction>,
    /// It reached its end value.
    pub end: Option<Interaction>,
    /// It was removed before its end: reversed, replaced, or its property
    /// no longer transitions.
    pub cancel: Option<Interaction>,
}

/// The messages a range input sends as its value changes: while it is
/// dragged (`data-on-input`) and once it is let go of with a new value
/// (`data-on-change`), after the DOM's events. Each is a message prefix the
/// value is appended to: `data-on-input="seek"` sends `seek:42`.
#[derive(Debug, Clone, Default)]
pub struct ValueHandlers {
    /// Sent with each new value while the input is dragged.
    pub input: Option<String>,
    /// Sent with the final value when the drag ends, if it changed.
    pub change: Option<String>,
}

impl ValueHandlers {
    pub(crate) fn is_empty(&self) -> bool {
        self.input.is_none() && self.change.is_none()
    }
}

impl TransitionHandlers {
    pub(crate) fn is_empty(&self) -> bool {
        self.run.is_none() && self.start.is_none() && self.end.is_none() && self.cancel.is_none()
    }
}

/// The core layout engine containing the taffy tree and metadata maps.
pub struct Ui {
    /// The taffy layout tree.
    pub taffy: TaffyTree,
    /// Resolved layout style and rendering properties per node.
    pub render_data: NodeMap<RenderData>,
    /// Mapped interaction identifier messages per node.
    pub interactions: NodeMap<Interaction>,
    /// Active scroll position offsets per scrollable node.
    pub scroll_offsets: NodeMap<(f32, f32)>,
    /// Root node identifier of the layout tree.
    pub root: NodeId,
    /// Associated HTML DOM tree handle per node.
    pub node_to_handle: NodeMap<Handle>,
    /// Cached layout styles and parsed attributes styles per node.
    pub base_styles: NodeMap<(Style, ContainerStyle)>,
    /// Parsed keyframe animations.
    pub keyframes: HashMap<String, css::KeyframesAnimation>,
    /// The `<html>` and `<body>` nodes, whose background paints the canvas.
    pub html: Option<NodeId>,
    /// See [`Ui::html`].
    pub body: Option<NodeId>,
    /// Shaped text of every text node, which layout measures and breaks.
    pub paragraphs: NodeMap<Arc<crate::text::ShapedText>>,
    /// Lines of every text node at its laid out width.
    pub text_lines: NodeMap<Vec<crate::text::Line>>,
    /// The shaped "…" of text nodes with `text-overflow: ellipsis`.
    pub ellipses: NodeMap<Arc<crate::text::ShapedText>>,
    /// The shaped text a text input shows: its value, or its placeholder.
    pub input_texts: NodeMap<Arc<crate::text::ShapedText>>,
    /// Elements with an `id` attribute, for [`Ui::scroll_into_view`].
    pub element_ids: Vec<(Cow<'static, str>, NodeId)>,
    /// Elements with a `lang` attribute, for [`Ui::lang`].
    pub langs: Vec<(NodeId, String)>,
    /// Each element's key, the same for the same element in every rebuild
    /// (see [`builder::element_key`]).
    pub keys: NodeMap<u64>,
    /// Whether any rule uses `:hover`, so that moving the pointer restyles.
    pub uses_hover: bool,
    /// The `data-on-hover` / `data-on-hover-out` messages of elements.
    pub hover_handlers: NodeMap<HoverHandlers>,
    /// The `data-on-transition*` messages of elements.
    pub transition_handlers: NodeMap<TransitionHandlers>,
    /// The `data-on-input` / `data-on-change` messages of range inputs.
    pub value_handlers: NodeMap<ValueHandlers>,
    /// Whether any rule uses `:active`.
    pub uses_active: bool,
    /// The styles elements that are new in this view start from, by
    /// `@starting-style` rules, for those that have any.
    pub starting_styles: NodeMap<(Style, ContainerStyle)>,
    /// Whether the template has `@starting-style` rules, so the next
    /// build computes starting styles for its new elements.
    pub uses_starting_style: bool,
}

impl Ui {
    /// Create a new UI layout engine by parsing an HTML source string.
    /// Available only when the `dynamic-parser` feature is enabled.
    #[cfg(feature = "dynamic-parser")]
    pub fn new(
        html: &str,
        measurer: &impl TextMeasurer,
        default_style: ContainerStyle,
        message_validator: &impl Fn(&str) -> bool,
    ) -> Result<Self, TaffyError> {
        let dom = parse_document(RcDom::default(), Default::default())
            .from_utf8()
            .read_from(&mut html.as_bytes())
            .unwrap();
        let mut css = String::new();
        html::extract_styles(&dom.document, &mut css);
        let mut builder = UiBuilder::new();
        let root = match html::build_document(&dom.document, &mut builder) {
            Some(root) => root,
            None => builder.create_element("html", &[]),
        };
        Self::from_builder(builder, root, &css, measurer, default_style, message_validator)
    }

    /// Create a new UI layout engine from a precompiled template layout.
    pub fn new_compiled(
        model: &impl TemplateLayout,
        measurer: &impl TextMeasurer,
        default_style: ContainerStyle,
        message_validator: &impl Fn(&str) -> bool,
    ) -> Result<Self, TaffyError> {
        Self::new_compiled_with_pointer(model, measurer, default_style, message_validator, PointerState::default())
    }

    /// [`Self::new_compiled`] with the elements under the pointer, which
    /// `:hover` and `:active` rules apply to.
    pub fn new_compiled_with_pointer(
        model: &impl TemplateLayout,
        measurer: &impl TextMeasurer,
        default_style: ContainerStyle,
        message_validator: &impl Fn(&str) -> bool,
        pointer: PointerState,
    ) -> Result<Self, TaffyError> {
        Self::new_compiled_after(model, measurer, default_style, message_validator, pointer, None)
    }

    /// Like [`Self::new_compiled_with_pointer`], for a view replacing one
    /// whose elements had `previous_keys`: the others are new, and get the
    /// styles of `@starting-style` rules computed. `None` computes none.
    pub fn new_compiled_after(
        model: &impl TemplateLayout,
        measurer: &impl TextMeasurer,
        default_style: ContainerStyle,
        message_validator: &impl Fn(&str) -> bool,
        pointer: PointerState,
        previous_keys: Option<HashSet<u64>>,
    ) -> Result<Self, TaffyError> {
        let mut builder = UiBuilder::new();
        builder.pointer = pointer;
        builder.previous_keys = previous_keys;
        let root = {
            model.build_ui(&mut builder, measurer, &default_style, message_validator)
        };
        let mut ui = Self::from_builder(builder, root, model.stylesheet(), measurer, default_style, message_validator)?;
        let (hover, active) = model.uses_pointer();
        ui.uses_hover |= hover;
        ui.uses_active |= active;
        Ok(ui)
    }

    /// The keys of the element at (`x`, `y`) and its ancestors, nearest
    /// first: what the pointer there hovers.
    pub fn pointer_chain(&self, x: f32, y: f32) -> Vec<u64> {
        let Some(node) = hit_test_recursive(&self.taffy, self.root, &self.scroll_offsets, &self.render_data, x, y, 0.0, 0.0) else {
            return Vec::new();
        };
        core::iter::successors(Some(node), |&n| self.taffy.parent(n)).filter_map(|n| self.keys.get(n).copied()).collect()
    }

    /// Styles the tree built with `builder` (template styles plus
    /// `stylesheet`, for elements built at runtime) and shapes its text.
    fn from_builder(
        mut builder: UiBuilder,
        root: NodeId,
        stylesheet: &str,
        measurer: &impl TextMeasurer,
        default_style: ContainerStyle,
        message_validator: &impl Fn(&str) -> bool,
    ) -> Result<Self, TaffyError> {
        let stylesheet_str = stylesheet;
        let mut base_styles = builder.base_styles;
        let mut keyframes = builder.keyframes;

        let (mut uses_hover, mut uses_active) = (false, false);
        if !stylesheet_str.is_empty() || builder.node_metadata.iter().next().is_some() {
            let cached = style_resolution::STYLESHEET_CACHE.with(|cache| {
                let mut cache_guard = cache.borrow_mut();
                if let Some(c) = cache_guard.get(stylesheet_str) {
                    c
                } else {
                    let keyframes = css::parse_keyframes(stylesheet_str);
                    let cached_val = std::rc::Rc::new(style_resolution::CachedStyles::new(stylesheet_str, keyframes));
                    cache_guard.insert(stylesheet_str.to_string(), cached_val.clone());
                    cached_val
                }
            });
            // The `@media` blocks matching the viewport this view is built for.
            let (width, height) = crate::screen::size();
            let media = cached.sheet.media_matches(width, height);

            if builder.node_metadata.get(root).is_some() && builder.keys.get(root).is_none() {
                builder.keys.insert(root, 0);
            }
            let root_key = builder.keys.get(root).copied().unwrap_or(0);
            assign_keys(&builder.taffy, root, root_key, &builder.node_metadata, &mut builder.keys);
            let mut style_cache = cached.style_cache.borrow_mut();
            // New elements get their `@starting-style` styles, as in a
            // compiled template.
            let has_starting = cached.sheet.rules.iter().any(|rule| rule.starting);
            builder.uses_starting_style |= has_starting;
            let mut starting = has_starting.then(|| style_resolution::Starting {
                previous_keys: builder.previous_keys.as_ref(),
                styles: &mut builder.starting_styles,
            });

            {
                style_resolution::resolve_styles(
                    &mut builder.taffy,
                    root,
                    measurer,
                    &mut builder.render_data,
                    &mut builder.interactions,
                    default_style,
                    message_validator,
                    &cached.sheet,
                    &media,
                    &builder.node_metadata,
                    &mut builder.keys,
                    &builder.pointer,
                    &mut builder.hover_handlers,
                    &mut builder.transition_handlers,
                    &mut builder.value_handlers,
                    &mut base_styles,
                    &mut *style_cache,
                    &mut starting,
                );
            }
            keyframes = cached.keyframes.clone();
            uses_hover = cached.sheet.uses_hover;
            uses_active = cached.sheet.uses_active;
        }

        let (html, body) = (builder.html, builder.body);
        let (paragraphs, ellipses, input_texts) = {
            shape_texts(&builder.render_data, measurer)
        };

        let mut element_ids = builder.element_ids;
        element_ids.extend(builder.node_metadata.iter().filter_map(|(node, meta)| Some((meta.id.clone()?, node))));
        let mut langs = builder.langs;
        langs.extend(builder.node_metadata.iter()
            .filter_map(|(node, meta)| Some((node, meta.other_attrs.as_ref()?.iter().find(|(k, _)| k == "lang")?.1.clone()))));
        Ok(Self {
            element_ids,
            langs,
            taffy: builder.taffy,
            render_data: builder.render_data,
            interactions: builder.interactions,
            scroll_offsets: NodeMap::new(),
            root,
            node_to_handle: builder.node_to_handle,
            base_styles,
            keyframes,
            html,
            body,
            paragraphs,
            text_lines: NodeMap::new(),
            ellipses,
            input_texts,
            keys: builder.keys,
            uses_hover,
            uses_active,
            hover_handlers: builder.hover_handlers,
            transition_handlers: builder.transition_handlers,
            value_handlers: builder.value_handlers,
            starting_styles: builder.starting_styles,
            uses_starting_style: builder.uses_starting_style,
        })
    }

    /// Scroll a specific node directly by delta_x and delta_y. Returns true if scroll offset changed.
    pub fn scroll_node(&mut self, node: NodeId, delta_x: f32, delta_y: f32) -> bool {
        if let Some(style) = self.render_data.get(&node).and_then(RenderData::container_style) {
            if style.overflow == Overflow::Scroll {
                 let (old_sx, old_sy) = self.scroll_offsets.get(&node).copied().unwrap_or((0.0, 0.0));
                 let mut sx = old_sx - delta_x;
                 let mut sy = old_sy - delta_y;
                 
                 if let Ok(layout) = self.taffy.layout(node) {
                     let container_width = layout.size.width;
                     let container_height = layout.size.height;
                     
                     let mut content_width = 0.0f32;
                     let mut content_height = 0.0f32;
                     
                     for child in self.taffy.child_ids(node) {
                         if let Ok(child_layout) = self.taffy.layout(child) {
                             let right = child_layout.location.x + child_layout.size.width;
                             let bottom = child_layout.location.y + child_layout.size.height;
                             if right > content_width { content_width = right; }
                             if bottom > content_height { content_height = bottom; }
                         }
                     }
                     
                     let max_sx = (content_width - container_width).max(0.0);
                     let max_sy = (content_height - container_height).max(0.0);
                     
                     sx = sx.clamp(0.0, max_sx);
                     sy = sy.clamp(0.0, max_sy);
                 }

                 self.scroll_offsets.insert(node, (sx, sy));
                 return sx != old_sx || sy != old_sy;
            }
        }
        false
    }

    /// Finds the scroll container at (x, y) if any.
    pub fn find_scroll_container(&self, x: f32, y: f32) -> Option<NodeId> {
        let mut curr = hit_test_recursive(&self.taffy, self.root, &self.scroll_offsets, &self.render_data, x, y, 0.0, 0.0)?;
        loop {
            if let Some(style) = self.render_data.get(&curr).and_then(RenderData::container_style) {
                if style.overflow == Overflow::Scroll {
                    return Some(curr);
                }
            }
            if let Some(parent) = self.taffy.parent(curr) {
                curr = parent;
            } else {
                return None;
            }
        }
    }

    /// Handles scroll inputs by checking for a scrollable element containing coordinates (x, y)
    /// and adjusting its scroll offset by (delta_x, delta_y). Returns true if scrolled.
    pub fn handle_scroll(&mut self, x: f32, y: f32, delta_x: f32, delta_y: f32) -> bool {
        if let Some(node) = self.find_scroll_container(x, y) {
            self.scroll_node(node, delta_x, delta_y)
        } else {
            false
        }
    }

    /// The language of `node`: its `lang` attribute or its nearest
    /// ancestor's, as in HTML. `None` when no element says.
    pub fn lang(&self, node: NodeId) -> Option<&str> {
        let mut current = Some(node);
        while let Some(node) = current {
            if let Some((_, lang)) = self.langs.iter().find(|(n, _)| *n == node) {
                return Some(lang);
            }
            current = self.taffy.parent(node);
        }
        None
    }

    /// Scrolls the nearest scrollable ancestor of the element with `id` so
    /// the element is visible.
    pub fn scroll_into_view(&mut self, id: &str) {
        let node_opt = self.element_ids.iter().find(|(element, _)| element == id).map(|(_, node)| *node);
        if let Some(node) = node_opt {
             let mut current = node;
             while let Some(parent) = self.taffy.parent(current) {
                 if let Some(style) = self.render_data.get(&parent).and_then(RenderData::container_style) {
                     if style.overflow == Overflow::Scroll {
                          if let Ok(parent_layout) = self.taffy.layout(parent) {
                              if let Ok(node_layout) = self.taffy.layout(node) {
                                   let (ck, cy) = self.scroll_offsets.get(&parent).copied().unwrap_or((0.0, 0.0));
                                   let node_y = node_layout.location.y; 
                                   
                                   let mut new_y = cy;
                                   if node_y < cy {
                                       new_y = node_y;
                                   } else if node_y + node_layout.size.height > cy + parent_layout.size.height {
                                       new_y = node_y + node_layout.size.height - parent_layout.size.height;
                                   }
                                   self.scroll_offsets.insert(parent, (ck, new_y));
                                   return;
                              }
                          }
                     }
                 }
                 current = parent;
             }
        }
    }

    /// Makes the root fill the definite axes of `available_space`.
    fn fill_available(&mut self, available_space: Size<AvailableSpace>) {
        let Ok(style) = self.taffy.style(self.root) else { return };
        let fill = |auto: bool, available: AvailableSpace| auto && matches!(available, AvailableSpace::Definite(_));
        let (fill_width, fill_height) =
            (fill(style.size.width.is_auto(), available_space.width), fill(style.size.height.is_auto(), available_space.height));
        if fill_width || fill_height {
            let mut style = style.clone();
            if fill_width {
                style.size.width = Dimension::percent(1.0);
            }
            if fill_height {
                style.size.height = Dimension::percent(1.0);
            }
            let _ = self.taffy.set_style(self.root, style);
        }
    }

    /// Takes over the Taffy tree of `old`, the view this one replaces, with
    /// its layout, when both have the same nodes under the root (as a view
    /// built again from the same template mostly has): the nodes whose
    /// style or text changed are set dirty in it, so [`Self::compute_layout`]
    /// lays out only those and their ancestors, and takes the rest from
    /// Taffy's cache. `old` gets this view's tree. Returns whether the tree
    /// was taken over.
    pub fn reuse_layout(&mut self, old: &mut Ui, available_space: Size<AvailableSpace>) -> bool {
        self.fill_available(available_space);
        if self.root != old.root || !same_children(&self.taffy, &old.taffy, self.root) {
            return false;
        }
        let mut stack = vec![self.root];
        while let Some(node) = stack.pop() {
            stack.extend(self.taffy.child_ids(node));
            let (Ok(style), Ok(old_style)) = (self.taffy.style(node), old.taffy.style(node)) else { continue };
            if style != old_style {
                let _ = old.taffy.set_style(node, style.clone());
            } else if !self.measures_like(old, node) {
                let _ = old.taffy.mark_dirty(node);
            }
        }
        core::mem::swap(&mut self.taffy, &mut old.taffy);
        true
    }

    /// Whether `node`, a text leaf here or in `other`, measures the same in
    /// both: same text, shaped the same, in the same line style.
    fn measures_like(&self, other: &Ui, node: NodeId) -> bool {
        match (self.paragraphs.get(node), other.paragraphs.get(node)) {
            (None, None) => true,
            (Some(paragraph), Some(other_paragraph)) => {
                let (Some(data), Some(other_data)) = (self.render_data.get(node), other.render_data.get(node)) else { return false };
                let (style, other_style) = (&data.style, &other_data.style);
                (Arc::ptr_eq(paragraph, other_paragraph) || paragraph == other_paragraph)
                    && style.font_size == other_style.font_size
                    && style.line_height == other_style.line_height
                    && style.white_space.wraps() == other_style.white_space.wraps()
                    && style.word_break() == other_style.word_break()
            }
            _ => false,
        }
    }

    /// Recalculates the taffy layout based on the available size constraints.
    pub fn compute_layout(&mut self, available_space: Size<AvailableSpace>) -> Result<(), TaffyError> {
        self.fill_available(available_space);
        let Ui { taffy, render_data, paragraphs, text_lines, ellipses, root, .. } = self;
        taffy.compute_layout_with_measure(*root, available_space, |known, available, node, _, _| {
            match (paragraphs.get(node), render_data.get(node)) {
                (Some(paragraph), Some(data)) => measure_paragraph(paragraph, &data.style, known, available),
                _ => Size::ZERO,
            }
        })?;
        // Break at the unrounded width: the one the text was measured at.
        text_lines.clear();
        for (node, paragraph) in paragraphs.iter() {
            let layout = taffy.unrounded_layout(node);
            let width = layout.content_box_width();
            let Some(style) = render_data.get(node).map(|d| &d.style) else { continue };
            let mut lines = paragraph.lines(wrap_width(style, width), style.word_break());
            if let Some(ellipsis) = ellipses.get(node) {
                // Lines must fit the clipping parent, which a text leaf can
                // be wider than (an anonymous flex item, say).
                let parent_room = taffy
                    .parent(node)
                    .map(|p| taffy.unrounded_layout(p))
                    .map_or(f32::INFINITY, |p| p.content_box_width() + p.padding.left + p.border.left - layout.location.x);
                let room = width.min(parent_room);
                for line in &mut lines {
                    *line = paragraph.truncate(*line, room, ellipsis.width());
                }
            }
            text_lines.insert(node, lines);
        }
        Ok(())
    }

    /// Traverses the layout tree and builds the hardware-agnostic rendering draw commands list.
    ///
    /// Like a browser, every frame starts from the canvas: the background of
    /// `<html>`, else of `<body>`, else white (dark with `color-scheme:
    /// dark`), so repainted regions never show the previous frame. The
    /// element whose background moved to the canvas doesn't paint it again,
    /// and when an opaque box already covers the viewport no canvas fill is
    /// emitted at all.
    pub fn build_commands(&self, _canvases: &HashMap<String, Canvas>, focused_id: Option<&str>) -> Vec<DrawCommand> {
        let source = [self.html, self.body].into_iter().flatten().find_map(|node| match self.render_data.get(&node).and_then(RenderData::container_style) {
            // Only a rendered element's background moves to the canvas.
            Some(style)
                if style.display != crate::style::Display::None
                    && (style.background_color.is_some() || style.background_gradient.is_some()) =>
            {
                Some((node, style))
            }
            _ => None,
        });

        let mut content = Vec::new();
        paint_layer(
            &Painter {
                taffy: &self.taffy,
                render_data: &self.render_data,
                paragraphs: &self.paragraphs,
                text_lines: &self.text_lines,
                ellipses: &self.ellipses,
                input_texts: &self.input_texts,
                scroll_offsets: &self.scroll_offsets,
                focused_id,
                canvas_source: source.map(|(node, _)| node),
            },
            &Deferred { node: self.root, offset_x: 0.0, offset_y: 0.0, clips: Vec::new(), z: 0 },
            &mut content,
        );

        let viewport = match self.taffy.layout(self.root) {
            Ok(l) => Rect { x: 0.0, y: 0.0, width: l.size.width, height: l.size.height },
            Err(_) => return content,
        };
        if covers_viewport(&content, viewport) {
            return content;
        }

        let canvas_rect = |color: Option<crate::Color>, gradient: Option<crate::Gradient>| DrawCommand::DrawRect {
            rect: viewport,
            color,
            gradient,
            border_radius: 0.0,
            border: crate::style::Borders::NONE,
        };
        let mut commands = Vec::with_capacity(content.len() + 2);
        let source_opaque = source.is_some_and(|(_, s)| is_opaque_fill(s.background_color, s.background_gradient.as_ref()));
        if !source_opaque {
            // The browser default canvas, under a translucent page background
            // or on its own.
            let dark = self.html.and_then(|n| self.render_data.get(&n)).is_some_and(|d| d.style.color_scheme_dark);
            let default = if dark { crate::Color::from_rgba8(0x12, 0x12, 0x12, 255) } else { crate::Color::WHITE };
            commands.push(canvas_rect(Some(default), None));
        }
        if let Some((_, style)) = source {
            commands.push(canvas_rect(style.background_color, style.background_gradient.clone()));
        }
        commands.extend(content);
        commands
    }

    /// The node of the element with `key`, if it is in the view.
    pub fn node_with_key(&self, key: u64) -> Option<NodeId> {
        self.keys.iter().find(|&(_, &k)| k == key).map(|(node, _)| node)
    }

    /// `node`'s border box on screen: its layout position plus its
    /// ancestors', less what scrolled ancestors scrolled by, as hit testing
    /// places it.
    pub fn absolute_rect(&self, node: NodeId) -> Option<Rect> {
        let layout = self.taffy.layout(node).ok()?;
        let (mut x, mut y) = (layout.location.x, layout.location.y);
        let mut current = node;
        while let Some(parent) = self.taffy.parent(current) {
            let parent_layout = self.taffy.layout(parent).ok()?;
            x += parent_layout.location.x;
            y += parent_layout.location.y;
            let scrolls = self.render_data.get(parent).is_some_and(|data| {
                matches!(data.kind, RenderKind::Container | RenderKind::TextInput(..)) && data.style.overflow == Overflow::Scroll
            });
            if let Some((sx, sy)) = self.scroll_offsets.get(parent).filter(|_| scrolls) {
                x -= sx;
                y -= sy;
            }
            current = parent;
        }
        Some(Rect { x, y, width: layout.size.width, height: layout.size.height })
    }

    /// The range input at (`x`, `y`) that sends messages as it changes
    /// (`data-on-input` / `data-on-change`), if there is one.
    pub fn range_at(&self, x: f32, y: f32) -> Option<NodeId> {
        let node = hit_test_recursive(&self.taffy, self.root, &self.scroll_offsets, &self.render_data, x, y, 0.0, 0.0)?;
        let is_range = matches!(self.render_data.get(node)?.kind, RenderKind::Slider(..));
        (is_range && self.value_handlers.get(node).is_some()).then_some(node)
    }

    /// The bounds and value of the range input `node`.
    pub fn range(&self, node: NodeId) -> Option<crate::style::Range> {
        match self.render_data.get(node)?.kind {
            RenderKind::Slider(range, _) => Some(range),
            _ => None,
        }
    }

    /// The value pressing the range input `node` at `x` gives: the one that
    /// puts the middle of its thumb there.
    pub fn range_value_at(&self, node: NodeId, x: f32) -> Option<f32> {
        let data = self.render_data.get(node)?;
        let RenderKind::Slider(range, parts) = &data.kind else { return None };
        let content = controls::content_box(self.absolute_rect(node)?, &data.style);
        Some(range.value_at(controls::slider_fraction_at(content, x, &data.style, parts.as_deref())))
    }

    /// Performs a hit test check for coordinates (x, y) and returns the corresponding interaction
    /// message identifier and the matching NodeId if found.
    pub fn hit_test(&self, x: f32, y: f32) -> Option<(Option<Interaction>, NodeId)> {
         if let Some(clicked_node) = hit_test_recursive(&self.taffy, self.root, &self.scroll_offsets, &self.render_data, x, y, 0.0, 0.0) {
             let mut current = Some(clicked_node);
             while let Some(node) = current {
                 if let Some(act) = self.interactions.get(&node) {
                     return Some((Some(act.clone()), clicked_node));
                 }
                 current = self.taffy.parent(node);
             }
             return Some((None, clicked_node));
         }
         None
    }
}

/// Whether the nodes under `node` are the same in `a` and `b`: the same
/// node ids, each with the same children in the same order.
fn same_children(a: &TaffyTree, b: &TaffyTree, node: NodeId) -> bool {
    let mut stack = vec![node];
    while let Some(node) = stack.pop() {
        if !a.child_ids(node).eq(b.child_ids(node)) {
            return false;
        }
        stack.extend(a.child_ids(node));
    }
    true
}

/// Gives the elements built at runtime under `node` (whose key is `key`)
/// their key: their parent's mixed with their index among its elements, or
/// with their `data-key`.
/// Template elements have theirs already.
fn assign_keys(taffy: &TaffyTree, node: NodeId, key: u64, metadata: &NodeMap<NodeMetadata>, keys: &mut NodeMap<u64>) {
    let Ok(children) = taffy.children(node) else { return };
    let mut index = 0;
    for child in children {
        let child_key = match keys.get(child) {
            Some(&child_key) => child_key,
            None if metadata.get(child).is_some_and(|m| m.tag != "#text") => {
                index += 1;
                let data_key = metadata.get(child).and_then(|m| m.attrs.iter().find(|(k, _)| k == "data-key"));
                let child_key = match data_key {
                    Some((_, value)) => builder::element_key(key, crate::model::hash_bytes(value.as_bytes()) ^ (1 << 63)),
                    None => builder::element_key(key, index),
                };
                keys.insert(child, child_key);
                child_key
            }
            None => continue,
        };
        assign_keys(taffy, child, child_key, metadata, keys);
    }
}

/// Applies `white-space` to the text of a text node: collapses whitespace
/// as `mode` says and expands tabs to 8-column stops.
pub fn collapse_white_space(text: &str, mode: crate::style::WhiteSpace) -> Cow<'_, str> {
    use crate::style::WhiteSpace;
    match mode {
        WhiteSpace::Normal | WhiteSpace::Nowrap => normalize_text(text),
        WhiteSpace::PreLine => {
            // Spaces collapse, newlines stay; spaces around a newline go.
            let lines: Vec<String> = text.split('\n').map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).collect();
            let joined = lines.join("\n");
            if joined == text { Cow::Borrowed(text) } else { Cow::Owned(joined) }
        }
        WhiteSpace::Pre | WhiteSpace::PreWrap => {
            if !text.contains(['\t', '\r']) {
                return Cow::Borrowed(text);
            }
            let mut out = String::with_capacity(text.len() + 8);
            let mut column = 0;
            for c in text.chars() {
                match c {
                    '\t' => {
                        let spaces = 8 - column % 8;
                        out.extend(core::iter::repeat_n(' ', spaces));
                        column += spaces;
                    }
                    '\r' => {}
                    '\n' => {
                        out.push(c);
                        column = 0;
                    }
                    _ => {
                        out.push(c);
                        column += 1;
                    }
                }
            }
            Cow::Owned(out)
        }
    }
}

/// Normalizes input text strings by collapsing multiple spaces and trimming leading/trailing spaces.
pub fn normalize_text(text: &str) -> Cow<'_, str> {
    let mut needs_normalization = false;
    let mut last_was_space = false;
    let mut is_first = true;
    for c in text.chars() {
        if c.is_whitespace() {
            if is_first || last_was_space || c != ' ' {
                needs_normalization = true;
                break;
            }
            last_was_space = true;
        } else {
            last_was_space = false;
        }
        is_first = false;
    }
    if last_was_space && !text.is_empty() {
        needs_normalization = true;
    }
    if needs_normalization {
        Cow::Owned(text.split_whitespace().collect::<Vec<&str>>().join(" "))
    } else {
        Cow::Borrowed(text)
    }
}

/// Trait for converting values to string types suitable for display within layout elements.
pub trait ToDisplayString {
    /// Converts the value to a Cow<'_, str> display string.
    fn to_display_string(&self) -> Cow<'_, str>;
}

impl ToDisplayString for str {
    fn to_display_string(&self) -> Cow<'_, str> {
        Cow::Borrowed(self)
    }
}

impl ToDisplayString for String {
    fn to_display_string(&self) -> Cow<'_, str> {
        Cow::Borrowed(self.as_str())
    }
}

impl<T: ToDisplayString + ?Sized> ToDisplayString for &T {
    fn to_display_string(&self) -> Cow<'_, str> {
        (*self).to_display_string()
    }
}

impl ToDisplayString for f32 {
    fn to_display_string(&self) -> Cow<'_, str> {
        Cow::Owned(self.to_string())
    }
}

impl ToDisplayString for f64 {
    fn to_display_string(&self) -> Cow<'_, str> {
        Cow::Owned(self.to_string())
    }
}

impl ToDisplayString for i32 {
    fn to_display_string(&self) -> Cow<'_, str> {
        Cow::Owned(self.to_string())
    }
}

impl ToDisplayString for u32 {
    fn to_display_string(&self) -> Cow<'_, str> {
        Cow::Owned(self.to_string())
    }
}

impl ToDisplayString for u64 {
    fn to_display_string(&self) -> Cow<'_, str> {
        Cow::Owned(self.to_string())
    }
}

impl ToDisplayString for i64 {
    fn to_display_string(&self) -> Cow<'_, str> {
        Cow::Owned(self.to_string())
    }
}

impl ToDisplayString for usize {
    fn to_display_string(&self) -> Cow<'_, str> {
        Cow::Owned(self.to_string())
    }
}

impl ToDisplayString for bool {
    fn to_display_string(&self) -> Cow<'_, str> {
        Cow::Borrowed(if *self { "true" } else { "false" })
    }
}

/// Whether a fill paints every pixel of its rect fully opaque.
fn is_opaque_fill(color: Option<crate::Color>, gradient: Option<&crate::Gradient>) -> bool {
    match gradient {
        Some(crate::Gradient::Linear(g)) => g.stops.iter().all(|(c, _)| c.a == 255),
        Some(crate::Gradient::Radial(g)) => g.stops.iter().all(|(c, _)| c.a == 255),
        Some(crate::Gradient::Conic(g)) => g.stops.iter().all(|(c, _)| c.a == 255),
        None => color.is_some_and(|c| c.a == 255),
    }
}

/// Whether some opaque, square-cornered rect paints the whole viewport, so
/// a canvas fill underneath would never be seen.
fn covers_viewport(commands: &[DrawCommand], viewport: Rect) -> bool {
    const SLACK: f32 = 0.5;
    let contains = |r: &Rect| {
        r.x <= viewport.x + SLACK
            && r.y <= viewport.y + SLACK
            && r.x + r.width >= viewport.x + viewport.width - SLACK
            && r.y + r.height >= viewport.y + viewport.height - SLACK
    };
    // Clips only shrink what a rect can cover; track whether all active
    // clips still contain the viewport.
    let mut clips: Vec<bool> = Vec::new();
    for cmd in commands {
        match cmd {
            DrawCommand::Clip { rect } => clips.push(contains(rect)),
            DrawCommand::PopClip => {
                clips.pop();
            }
            DrawCommand::DrawRect { rect, color, gradient, border_radius, .. } => {
                if *border_radius <= 0.0
                    && contains(rect)
                    && clips.iter().all(|&c| c)
                    && is_opaque_fill(*color, gradient.as_ref())
                {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// What painting needs from the `Ui`.
struct Painter<'a> {
    taffy: &'a TaffyTree,
    render_data: &'a NodeMap<RenderData>,
    paragraphs: &'a NodeMap<Arc<crate::text::ShapedText>>,
    text_lines: &'a NodeMap<Vec<crate::text::Line>>,
    ellipses: &'a NodeMap<Arc<crate::text::ShapedText>>,
    input_texts: &'a NodeMap<Arc<crate::text::ShapedText>>,
    scroll_offsets: &'a NodeMap<(f32, f32)>,
    focused_id: Option<&'a str>,
    canvas_source: Option<NodeId>,
}

/// A positioned element, painted after the in-flow content of its stacking
/// context, inside the clips of its ancestors.
struct Deferred {
    node: NodeId,
    offset_x: f32,
    offset_y: f32,
    clips: Vec<Rect>,
    z: i32,
}

/// The paint layer of `node` in a parent laid out as `parent_is_flex`:
/// `Some(z)` for a positioned element or a flex item with a `z-index`
/// (`auto` counts as 0), or an element with `opacity` below 1 (painted like
/// a positioned one with `z-index: 0`, CSS 2 Appendix E); `None` for
/// in-flow content.
fn paint_layer_of(node: NodeId, parent_is_flex: bool, render_data: &NodeMap<RenderData>) -> Option<i32> {
    let data = render_data.get(&node)?;
    let translucent = data.style.opacity < 1.0;
    let Some(style) = data.container_style() else { return translucent.then_some(0) };
    let positioned = style.position != crate::style::Position::Static;
    match style.z_index {
        Some(z) if positioned || parent_is_flex => Some(z),
        _ if positioned || translucent => Some(0),
        _ => None,
    }
}

/// Paints `layer.node` as a stacking context, in CSS order: its own box,
/// positioned descendants with a negative `z-index`, its in-flow content,
/// positioned descendants with `z-index: auto` or 0 in tree order, then
/// positive `z-index`. Each positioned descendant is painted the same way;
/// unlike CSS, one with `z-index: auto` keeps its positioned descendants to
/// itself.
///
/// A layer with `opacity` below 1 fades each of its commands. Unlike a
/// browser, which composites the layer as one image, parts of it that
/// overlap show through each other.
fn paint_layer(painter: &Painter, layer: &Deferred, commands: &mut Vec<DrawCommand>) {
    for &rect in &layer.clips {
        commands.push(DrawCommand::Clip { rect });
    }
    let start = commands.len();
    let mut own = Vec::new();
    let mut clips = layer.clips.clone();
    let mut deferred = Vec::new();
    let box_end = traverse_layout(painter, layer.node, layer.offset_x, layer.offset_y, &mut own, &mut clips, &mut deferred);
    deferred.sort_by_key(|d| d.z); // stable: ties keep tree order
    let mut rest = own.split_off(box_end);
    commands.append(&mut own);
    let below = deferred.iter().take_while(|d| d.z < 0).count();
    for d in &deferred[..below] {
        paint_layer(painter, d, commands);
    }
    commands.append(&mut rest);
    for d in &deferred[below..] {
        paint_layer(painter, d, commands);
    }
    let opacity = painter.render_data.get(&layer.node).map_or(1.0, |data| data.style.opacity);
    if opacity <= 0.0 {
        commands.truncate(start);
    } else if opacity < 1.0 {
        commands[start..].iter_mut().for_each(|command| command.fade(opacity));
    }
    for _ in &layer.clips {
        commands.push(DrawCommand::PopClip);
    }
}

/// Paints `root` and its in-flow descendants; positioned descendants go to
/// `deferred`. Returns the length `commands` had after `root`'s own box.
fn traverse_layout(
    painter: &Painter,
    root: NodeId,
    offset_x: f32,
    offset_y: f32,
    commands: &mut Vec<DrawCommand>,
    clips: &mut Vec<Rect>,
    deferred: &mut Vec<Deferred>,
) -> usize {
    let Painter { taffy, render_data, paragraphs, text_lines, ellipses, input_texts, scroll_offsets, focused_id, canvas_source } = *painter;
    let layout = match taffy.layout(root) {
        Ok(l) => l,
        Err(_) => return commands.len(),
    };
    
    let x = offset_x + layout.location.x;
    let y = offset_y + layout.location.y;
    let width = layout.size.width;
    let height = layout.size.height;

    let mut overflow = Overflow::Visible;
    let rect = Rect { x, y, width, height };

    if let Some(data) = render_data.get(&root) {
        let style = &data.style;
        if style.display == crate::style::Display::None {
            return commands.len();
        }
        // Boxes painted from their CSS. A checkbox's native look ignores
        // its background and border, as in Chromium.
        let maybe_style = match data.kind {
            RenderKind::Container | RenderKind::Text(_) | RenderKind::Slider(..) | RenderKind::Progress(..) => Some(style),
            RenderKind::Checkbox(_) if style.appearance_none => Some(style),
            _ => None,
        };

        if let Some(style) = maybe_style {
            overflow = style.overflow;

            // An outer shadow is under the background; an inset one is
            // above it, inside the padding box (CSS Backgrounds 3, 7.1).
            let shadow = |bs: &crate::style::BoxShadow, rect: Rect, border_radius: f32| DrawCommand::DrawBoxShadow {
                rect,
                border_radius,
                offset_x: bs.offset_x,
                offset_y: bs.offset_y,
                blur_radius: bs.blur_radius,
                spread_radius: bs.spread_radius,
                color: bs.color,
                inset: bs.inset,
            };
            if let Some(bs) = style.box_shadow.as_ref().filter(|bs| !bs.inset) {
                commands.push(shadow(bs, rect, style.border_radius));
            }

            // A background propagated to the canvas is painted there instead.
            let on_canvas = canvas_source == Some(root);
            let (bg_color, bg_gradient) = if on_canvas { (None, None) } else { (style.background_color, style.background_gradient.clone()) };
            if bg_color.is_some() || bg_gradient.is_some() || style.border.is_visible() {
                 commands.push(DrawCommand::DrawRect {
                    rect,
                    color: bg_color,
                    gradient: bg_gradient,
                    border_radius: style.border_radius,
                    border: style.border.resolved(style.color),
                });
            }
            if let Some(bs) = style.box_shadow.as_ref().filter(|bs| bs.inset) {
                let b = &style.border;
                let (top, right, bottom, left) = (b.top.used_width(), b.right.used_width(), b.bottom.used_width(), b.left.used_width());
                let padding_box = Rect::new(rect.x + left, rect.y + top, (rect.width - left - right).max(0.0), (rect.height - top - bottom).max(0.0));
                let radius = (style.border_radius - top.max(right).max(bottom).max(left)).max(0.0);
                commands.push(shadow(bs, padding_box, radius));
            }
        }

        match &data.kind {
            RenderKind::Text(text) => {
                if let (Some(paragraph), Some(lines)) = (paragraphs.get(root), text_lines.get(root)) {
                    push_text_lines(commands, text, style, paragraph, lines, ellipses.get(root), rect);
                }
            },
            RenderKind::Image(src) => {
                 commands.push(DrawCommand::DrawImage {
                    src: src.clone(),
                    rect,
                    border_radius: style.border_radius,
                    opacity: 1.0,
                });
            },
            RenderKind::Checkbox(checked) => controls::checkbox(commands, rect, *checked, style),
            RenderKind::Slider(range, parts) => {
                controls::slider(commands, controls::content_box(rect, style), range.fraction(), style, parts.as_deref());
            }
            RenderKind::Progress(value, max, parts) => {
                let fraction = if *max > 0.0 { value / max } else { 0.0 };
                controls::progress(commands, controls::content_box(rect, style), fraction, style, parts.as_deref());
            }
            RenderKind::Canvas(id) => {
                let b = style.border.widths();
                let canvas_rect = if style.border.is_visible() {
                    Rect {
                        x: rect.x + b.left,
                        y: rect.y + b.top,
                        width: (rect.width - style.border.horizontal_width()).max(1.0),
                        height: (rect.height - style.border.vertical_width()).max(1.0),
                    }
                } else {
                    rect
                };
                commands.push(DrawCommand::DrawCanvas {
                    id: id.clone(),
                    rect: canvas_rect,
                    border_radius: style.border_radius,
                    opacity: 1.0,
                });
            },
            RenderKind::TextInput(id, text, placeholder) => {
                let is_focused = focused_id == Some(id.as_str()) && !id.is_empty();
                
                let focus_ring = crate::style::BorderSide::solid(2.0, crate::Color::from_rgba8(0, 122, 255, 255));
                commands.push(DrawCommand::DrawRect {
                    rect,
                    color: style.background_color,
                    gradient: style.background_gradient.clone(),
                    border_radius: style.border_radius,
                    border: if is_focused { crate::style::Borders::all(focus_ring) } else { style.border.resolved(style.color) },
                });

                if let Some((t, text_style)) = input_shown(text, placeholder, style) {
                    let shaped = input_texts.get(root);
                    // Inset by the left padding and center one line vertically.
                    let pad_x = if style.padding_left > 0.0 { style.padding_left } else { 8.0 };
                    // Taller for a fallback font's line (Hangul), as text elements.
                    let normal = shaped.map_or(0.0, |s| s.metrics.normal_line_height());
                    let line_h = (text_style.font_size * 1.2).max(normal);
                    let text_rect = Rect {
                        x: rect.x + pad_x,
                        y: rect.y + ((rect.height - line_h) * 0.5).max(0.0),
                        width: (rect.width - 2.0 * pad_x).max(0.0),
                        height: line_h.min(rect.height),
                    };
                    commands.push(DrawCommand::DrawText {
                        text: t.to_string(),
                        rect: text_rect,
                        color: text_style.color,
                        font_size: text_style.font_size,
                        weight: text_style.weight,
                        run: shaped.map(|shaped| crate::text::GlyphRun {
                            shaped: shaped.clone(),
                            start: 0,
                            end: shaped.glyphs.len() as u32,
                            text_start: 0,
                            // The line's half-leading above the ascent.
                            baseline: shaped.metrics.ascent + ((line_h - normal) * 0.5).max(0.0),
                        }),
                    });
                }
            },
            _ => {} 
        }
    }

    let box_end = commands.len();
    if overflow != Overflow::Visible {
        commands.push(DrawCommand::Clip { rect });
        clips.push(rect);
    }

    let mut child_offset_x = x;
    let mut child_offset_y = y;

    if overflow == Overflow::Scroll {
        if let Some((sx, sy)) = scroll_offsets.get(&root) {
             child_offset_x -= sx;
             child_offset_y -= sy;
        }
    }

    let is_flex = render_data.get(&root).and_then(RenderData::container_style).is_some_and(|s| s.display.is_flex());
    for child in taffy.child_ids(root) {
        match paint_layer_of(child, is_flex, render_data) {
            Some(z) => deferred.push(Deferred { node: child, offset_x: child_offset_x, offset_y: child_offset_y, clips: clips.clone(), z }),
            None => { traverse_layout(painter, child, child_offset_x, child_offset_y, commands, clips, deferred); }
        }
    }

    if overflow != Overflow::Visible {
        commands.push(DrawCommand::PopClip);
        clips.pop();
    }
    box_end
}

/// Orders `children` the way [`paint_layer`] paints them, for hit testing:
/// negative `z-index`, in-flow, `auto`/0, positive. Only siblings are
/// ordered here.
fn paint_order(mut children: Vec<NodeId>, parent: NodeId, render_data: &NodeMap<RenderData>) -> Vec<NodeId> {
    let parent_is_flex = render_data.get(&parent).and_then(RenderData::container_style).is_some_and(|s| s.display.is_flex());
    let layer = |child: &NodeId| match paint_layer_of(*child, parent_is_flex, render_data) {
        None => (1, 0),
        Some(z) if z < 0 => (0, z),
        Some(z) => (2, z),
    };
    if children.iter().any(|c| layer(c) != (1, 0)) {
        children.sort_by_key(layer); // stable: ties keep tree order
    }
    children
}

/// Style of a text node in an element styled `parent_style`: the
/// inherited text properties, without the parent's box decorations.
pub(crate) fn text_style(parent_style: &ContainerStyle) -> ContainerStyle {
    let mut style = parent_style.clone();
    style.background_color = None;
    style.background_gradient = None;
    style.box_shadow = None;
    style.border = crate::style::Borders::NONE;
    style.border_radius = 0.0;
    // An ellipsis needs the parent to clip what overflows.
    style.text_overflow_ellipsis = parent_style.text_overflow_ellipsis && parent_style.overflow != Overflow::Visible;
    style.overflow = Overflow::Visible;
    style.animation_name = None;
    style.animation_duration = 0.0;
    style.animation_timing_function = crate::css::animation::Easing::EASE;
    style.animation_delay = 0.0;
    style.animation_iteration_count = crate::style::AnimationIterationCount::Count(1.0);
    style.animation_direction = "normal";
    style.animation_fill_mode = "none";
    style.animation_play_state = "running";
    style.transitions = None;
    style
}

/// Layout style of a text leaf whose parent has `parent_style` (the text's
/// own style is inherited from it). Its size comes from the measure
/// function.
///
/// In a block container the leaf stands in for the container's lines, which
/// are as wide as the container whatever the text's min-content width:
/// `min-width: 0` lets it shrink to that width, so words break where they
/// may and otherwise overflow, as in a browser. Text directly in a flex
/// container is an anonymous flex item and keeps the automatic minimum.
pub(crate) fn text_leaf_style(parent_style: &ContainerStyle) -> Style {
    let mut style = Style::default();
    if !parent_style.display.is_flex() {
        style.min_size.width = Dimension::length(0.0);
    }
    style
}

/// Shaped text node contents, by node.
type ShapedTexts = NodeMap<Arc<crate::text::ShapedText>>;

/// Shapes the text of every text node, and the "…" of those that may need
/// one.
fn shape_texts(render_data: &NodeMap<RenderData>, measurer: &impl TextMeasurer) -> (ShapedTexts, ShapedTexts, ShapedTexts) {
    let mut paragraphs = NodeMap::new();
    let mut ellipses = NodeMap::new();
    let mut input_texts = NodeMap::new();
    for (node, data) in render_data.iter() {
        match &data.kind {
            RenderKind::Text(text) => {
                let style = &data.style;
                paragraphs.insert(node, measurer.shape(text, style.font_size, style.weight, style.font_features));
                if style.text_overflow_ellipsis {
                    ellipses.insert(node, measurer.shape("\u{2026}", style.font_size, style.weight, style.font_features));
                }
            }
            RenderKind::TextInput(_, value, placeholder) => {
                if let Some((text, style)) = input_shown(value, placeholder, &data.style) {
                    input_texts.insert(node, measurer.shape(text, style.font_size, style.weight, style.font_features));
                }
            }
            _ => {}
        }
    }
    (paragraphs, ellipses, input_texts)
}

/// What a text input shows, in which style: its value, or its placeholder
/// while the value is empty.
fn input_shown<'a>(
    value: &'a Option<String>,
    placeholder: &'a Option<Box<(String, ContainerStyle)>>,
    style: &'a ContainerStyle,
) -> Option<(&'a str, &'a ContainerStyle)> {
    match (value.as_deref(), placeholder.as_deref()) {
        (Some(t), _) if !t.is_empty() => Some((t, style)),
        (_, Some((p, p_style))) => Some((p.as_str(), p_style)),
        _ => None,
    }
}

/// Height of one line of `paragraph`.
fn line_height(paragraph: &Arc<crate::text::ShapedText>, style: &ContainerStyle) -> f32 {
    style.line_height.resolve(style.font_size, paragraph.metrics.normal_line_height())
}

/// The width lines of text in `style` are broken at, for a box `width` wide.
fn wrap_width(style: &ContainerStyle, width: f32) -> f32 {
    if style.white_space.wraps() { width } else { f32::INFINITY }
}

/// Taffy measure function for a text leaf: CSS min/max-content widths, and
/// the height of the text wrapped at the width taffy settles on.
fn measure_paragraph(
    paragraph: &Arc<crate::text::ShapedText>,
    style: &ContainerStyle,
    known: Size<Option<f32>>,
    available: Size<AvailableSpace>,
) -> Size<f32> {
    if paragraph.glyphs.is_empty() {
        return Size::ZERO;
    }
    let wraps = style.white_space.wraps();
    let min_content = if wraps { paragraph.min_content_width(style.word_break()) } else { paragraph.max_content_width() };
    let width = known.width.unwrap_or_else(|| match available.width {
        AvailableSpace::MinContent => min_content,
        AvailableSpace::MaxContent => paragraph.max_content_width(),
        AvailableSpace::Definite(w) => w.min(paragraph.max_content_width()).max(min_content),
    });
    let height = known
        .height
        .unwrap_or_else(|| paragraph.line_count(wrap_width(style, width), style.word_break()) as f32 * line_height(paragraph, style));
    Size { width, height }
}

/// Emits one `DrawText` per line of a laid out text node at `rect`, plus
/// one for each line's ellipsis.
fn push_text_lines(
    commands: &mut Vec<DrawCommand>,
    text: &str,
    style: &ContainerStyle,
    paragraph: &Arc<crate::text::ShapedText>,
    lines: &[crate::text::Line],
    ellipsis: Option<&Arc<crate::text::ShapedText>>,
    rect: Rect,
) {
    let line_h = line_height(paragraph, style);
    let m = paragraph.metrics;
    // Half the leading goes above the text (CSS 2 10.8.1), floored like
    // Chromium so odd leading puts the extra pixel below.
    let baseline = crate::graphics::floor((line_h - (m.ascent + m.descent)) / 2.0) + m.ascent;
    for (i, line) in lines.iter().enumerate() {
        let ellipsis = ellipsis.filter(|_| line.ellipsis);
        let ellipsis_width = ellipsis.map_or(0.0, |e| e.width());
        let y = rect.y + i as f32 * line_h;
        // Lines too wide for the box stay at the start edge.
        let free = (rect.width - line.width - ellipsis_width).max(0.0);
        let offset = match style.text_align {
            Some(crate::style::TextAlign::Center) => free / 2.0,
            Some(crate::style::TextAlign::Right) => free,
            _ => 0.0,
        };
        if line.end > line.start {
            push_line(commands, text, style, paragraph, line, Rect { x: rect.x + offset, y, width: line.width, height: line_h }, baseline);
        }
        if let Some(ellipsis) = ellipsis {
            commands.push(DrawCommand::DrawText {
                text: "\u{2026}".to_string(),
                rect: Rect { x: rect.x + offset + line.width, y, width: ellipsis_width, height: line_h },
                color: style.color,
                font_size: style.font_size,
                weight: style.weight,
                run: Some(crate::text::GlyphRun {
                    shaped: ellipsis.clone(),
                    start: 0,
                    end: ellipsis.glyphs.len() as u32,
                    text_start: 0,
                    baseline,
                }),
            });
        }
    }
}

/// Emits the `DrawText` of one line of `paragraph`.
fn push_line(
    commands: &mut Vec<DrawCommand>,
    text: &str,
    style: &ContainerStyle,
    paragraph: &Arc<crate::text::ShapedText>,
    line: &crate::text::Line,
    rect: Rect,
    baseline: f32,
) {
    let glyphs = &paragraph.glyphs;
    // From the first glyph's character to the end of the last glyph's,
    // leaving out newlines and hanging spaces.
    let start = glyphs[line.start as usize].cluster as usize;
    let last = glyphs[line.end as usize - 1].cluster as usize;
    let end = last + text[last..].chars().next().map_or(0, char::len_utf8);
    commands.push(DrawCommand::DrawText {
        text: text[start..end].to_string(),
        rect,
        color: style.color,
        font_size: style.font_size,
        weight: style.weight,
        run: Some(crate::text::GlyphRun {
            shaped: paragraph.clone(),
            start: line.start,
            end: line.end,
            text_start: start as u32,
            baseline,
        }),
    });
}

/// Recursively traverses children nodes to hit-test a point against layout boundaries.
pub fn hit_test_recursive(
    taffy: &TaffyTree,
    root: NodeId,
    scroll_offsets: &NodeMap<(f32, f32)>,
    render_data: &NodeMap<RenderData>,
    x: f32,
    y: f32,
    abs_x: f32,
    abs_y: f32,
) -> Option<NodeId> {
    let layout = taffy.layout(root).ok()?;
    let left = abs_x + layout.location.x;
    let top = abs_y + layout.location.y;
    let width = layout.size.width;
    let height = layout.size.height;

    let inside = x >= left && x <= left + width && y >= top && y <= top + height;
    let mut child_abs_x = left;
    let mut child_abs_y = top;
    let mut overflow = Overflow::Visible;
    if let Some(data) = render_data.get(&root) {
        // Like painting: no box, no hit.
        if data.style.display == crate::style::Display::None {
            return None;
        }
        if let RenderKind::Container | RenderKind::TextInput(..) = data.kind {
            overflow = data.style.overflow;
        }
        if overflow == Overflow::Scroll {
            if let Some((sx, sy)) = scroll_offsets.get(&root) {
                child_abs_x -= sx;
                child_abs_y -= sy;
            }
        }
    }
    // Like painting, children overflowing the box are hit too, unless the
    // box clips them: absolutely positioned elements in a body of no
    // height, a dropdown hanging out of its button.
    if !inside && overflow != Overflow::Visible {
        return None;
    }

    if let Ok(children) = taffy.children(root) {
         for child in paint_order(children, root, render_data).iter().rev() {
             if let Some(hit) = hit_test_recursive(taffy, *child, scroll_offsets, render_data, x, y, child_abs_x, child_abs_y) {
                 return Some(hit);
             }
         }
    }
    inside.then_some(root)
}
