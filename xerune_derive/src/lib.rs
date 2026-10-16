use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::cell::RefCell;
use xerune_template::{Attr, AttrPart, Cond, Expr, Kind, NodeId, Pattern, Template};
use xerune_css::selector::PseudoElement;

// ---------------------------------------------------------------------------
// Diagnostics
//
// The template compiler is deeply recursive and reports problems by panicking
// with a `TemplateError` payload (see `fail`). `derive_xerune_template` catches
// the unwind and turns it into a regular `compile_error!`, so users see a
// `file:line:col` message instead of "proc-macro derive panicked".
// ---------------------------------------------------------------------------

/// Panic payload for user-facing template errors.
struct TemplateError(String);

thread_local! {
    /// The next element or loop's place in the template, for element keys.
    static NEXT_SITE: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// A number for the next element or loop of the template being compiled,
/// which its elements' keys mix in (`xerune::__private::element_key`).
fn next_site() -> u64 {
    NEXT_SITE.with(|site| {
        site.set(site.get() + 1);
        site.get()
    })
}

/// Abort template compilation with `msg`, without a source location.
fn fail(msg: impl std::fmt::Display) -> ! {
    std::panic::panic_any(TemplateError(msg.to_string()))
}

// Stringify patterns.
fn format_target(target: &Pattern) -> String {
    match target {
        Pattern::Name(name) => name.to_string(),
        Pattern::Placeholder => "_".to_string(),
        Pattern::Path(path) => path.join("::"),
        Pattern::Tuple(path, targets) => {
            let inner: Vec<String> = targets.iter().map(format_target).collect();
            match (path.is_empty(), inner.len()) {
                // A one-element tuple pattern needs the trailing comma.
                (true, 1) => format!("({},)", inner[0]),
                (true, _) => format!("({})", inner.join(", ")),
                (false, _) => format!("{}({})", path.join("::"), inner.join(", ")),
            }
        }
        Pattern::Rest(_) => "..".to_string(),
    }
}

/// Collect the variable names a pattern binds, so the body treats them as
/// locals rather than model fields (`self.name`).
fn target_names(target: &Pattern, names: &mut HashSet<String>) {
    let mut bound = Vec::new();
    target.names(&mut bound);
    names.extend(bound);
}

/// A piece of an inline text run: literal text or a `{{ }}` value.
enum TextPiece {
    Literal(String),
    Expr(proc_macro2::TokenStream),
}

/// Node `id` as part of a text run, if it is text or a `{{ }}` value.
fn text_piece(tpl: &Template, id: NodeId, local_vars: &HashSet<String>) -> Option<TextPiece> {
    match &tpl.node(id).kind {
        Kind::Text(text) => Some(TextPiece::Literal(text.clone())),
        Kind::Expr(expr) => {
            let tokens: proc_macro2::TokenStream = format_expr(expr, local_vars).parse().unwrap();
            Some(TextPiece::Expr(tokens))
        }
        _ => None,
    }
}

/// Compiles sibling nodes. Consecutive text and `{{ }}` values form one
/// text node, like one run of text in a browser, so it wraps as a whole
/// and keeps the spaces between its pieces.
fn compile_children(
    tpl: &Template,
    children: &[NodeId],
    rules: &[GuardedRule],
    local_vars: &mut HashSet<String>,
) -> Vec<proc_macro2::TokenStream> {
    let mut code = Vec::new();
    let mut run: Vec<TextPiece> = Vec::new();
    let flush = |run: &mut Vec<TextPiece>, code: &mut Vec<proc_macro2::TokenStream>| {
        let pieces = core::mem::take(run);
        let has_content = pieces.iter().any(|p| match p {
            TextPiece::Literal(t) => !t.trim().is_empty(),
            TextPiece::Expr(_) => true,
        });
        if !has_content {
            return;
        }
        if let [TextPiece::Literal(text)] = pieces.as_slice() {
            code.push(quote! { builder.append_text_node(measurer, parent, &parent_style, #text); });
            return;
        }
        let pushes = pieces.iter().map(|p| match p {
            TextPiece::Literal(t) => quote! { text_run.push_str(#t); },
            TextPiece::Expr(e) => quote! { text_run.push_str(&(#e).to_display_string()); },
        });
        code.push(quote! {
            {
                use ::xerune::__private::ToDisplayString;
                let mut text_run = String::new();
                #(#pushes)*
                builder.append_text_node(measurer, parent, &parent_style, &text_run);
            }
        });
    };
    for &child in children {
        if let Some(piece) = text_piece(tpl, child, local_vars) {
            run.push(piece);
            continue;
        }
        flush(&mut run, &mut code);
        code.push(compile_node(tpl, child, rules, local_vars));
    }
    flush(&mut run, &mut code);
    code
}

// Stringify expressions as Rust.
fn format_expr(expr: &Expr, local_vars: &HashSet<String>) -> String {
    match expr {
        Expr::Bool(b) => format!("{}", b),
        Expr::Num(s) => s.to_string(),
        Expr::Str(s) => format!("\"{}\"", s),
        Expr::Char(c) => format!("'{}'", c),
        Expr::Var(name) => {
            if name == "self" {
                "self".to_string()
            } else if local_vars.contains(name) {
                name.to_string()
            } else {
                format!("self.{}", name)
            }
        }
        Expr::Path(parts) => parts.join("::"),
        Expr::Array(elems) => {
            let inner: Vec<String> = elems.iter().map(|e| format_expr(e, local_vars)).collect();
            format!("[{}]", inner.join(", "))
        }
        Expr::Attr(obj, attr) => {
            let obj_str = format_expr(obj, local_vars);
            if obj_str == "loop" {
                match attr.as_str() {
                    "index" => "(_loop_item_index + 1)".to_string(),
                    "index0" => "_loop_item_index".to_string(),
                    _ => panic!("Unsupported loop attribute: {}", attr),
                }
            } else {
                format!("{}.{}", obj_str, attr)
            }
        }
        Expr::Index(obj, key) => {
            format!("{}[{}]", format_expr(obj, local_vars), format_expr(key, local_vars))
        }
        Expr::Unary(op, inner) => {
            format!("{}{}", op.as_str(), format_expr(inner, local_vars))
        }
        Expr::Binary(op, left, right) => {
            format!("{} {} {}", format_expr(left, local_vars), op.as_str(), format_expr(right, local_vars))
        }
        Expr::Range(op, left, right) => {
            let left = left.as_ref().map(|e| format_expr(e, local_vars)).unwrap_or_default();
            let right = right.as_ref().map(|e| format_expr(e, local_vars)).unwrap_or_default();
            format!("({}{}{})", left, op.as_str(), right)
        }
        Expr::Group(inner) => {
            format!("({})", format_expr(inner, local_vars))
        }
        Expr::Tuple(elems) => {
            let inner: Vec<String> = elems.iter().map(|e| format_expr(e, local_vars)).collect();
            format!("({})", inner.join(", "))
        }
        Expr::Call(path, args) => {
            let inner: Vec<String> = args.iter().map(|e| format_expr(e, local_vars)).collect();
            format!("{}({})", format_expr(path, local_vars), inner.join(", "))
        }
        Expr::Format(args) => {
            let fmt_str = format_expr(&args[0], local_vars);
            let args: Vec<String> = args[1..].iter().map(|arg| format_expr(arg, local_vars)).collect();
            format!("format!({}, {})", fmt_str, args.join(", "))
        }
    }
}

fn format_cond_test(cond: &Cond, local_vars: &HashSet<String>) -> String {
    let expr_str = format_expr(&cond.expr, local_vars);
    if let Some(ref target) = cond.pattern {
        // Borrow the scrutinee: templates only get `&self`, so binding by
        // value would try to move out of the model.
        format!("let {} = &({})", format_target(target), expr_str)
    } else {
        as_bool(&expr_str)
    }
}

/// `expr` as a `bool`, accepting `&bool` too: loop variables are references
/// into the iterated collection, so `{% for on in flags %}{% if on %}` sees
/// a `&bool`.
fn as_bool(expr: &str) -> String {
    format!("(*::core::borrow::Borrow::<bool>::borrow(&({})))", expr)
}

/// A condition as a plain `bool` expression (usable in `&&` chains and under
/// `!`). `if let PAT = EXPR` becomes `matches!(&(EXPR), PAT)`.
fn cond_test_as_bool(cond: &Cond, local_vars: &HashSet<String>) -> String {
    let expr_str = format_expr(&cond.expr, local_vars);
    match &cond.pattern {
        Some(target) => format!(
            "{{ #[allow(unused_variables)] let m = matches!(&({}), {}); m }}",
            expr_str,
            format_target(target)
        ),
        None => as_bool(&expr_str),
    }
}

// Generate code for the parts of an attribute value
fn generate_attr_string_code(parts: &[AttrPart], local_vars: &HashSet<String>) -> proc_macro2::TokenStream {
    let has_complex = parts.iter().any(|part| matches!(part, AttrPart::If(_)));
    if !has_complex && !parts.is_empty() {
        let mut format_str = String::new();
        let mut format_args = Vec::new();
        for part in parts {
            match part {
                AttrPart::Lit(val) => {
                    let escaped = val.replace("{", "{{").replace("}", "}}");
                    format_str.push_str(&escaped);
                }
                AttrPart::Expr(expr) => {
                    let expr_str = format_expr(expr, local_vars);
                    let expr_tokens: proc_macro2::TokenStream = expr_str.parse().unwrap();
                    format_str.push_str("{}");
                    format_args.push(expr_tokens);
                }
                AttrPart::If(_) => unreachable!(),
            }
        }
        return quote! {
            format!(#format_str, #(#format_args),*)
        };
    }

    let mut code = Vec::new();
    for part in parts {
        match part {
            AttrPart::Lit(val) => {
                code.push(quote! { #val });
            }
            AttrPart::Expr(expr) => {
                let expr_str = format_expr(expr, local_vars);
                let expr_tokens: proc_macro2::TokenStream = expr_str.parse().unwrap();
                code.push(quote! { &*format!("{}", #expr_tokens) });
            }
            AttrPart::If(branches) => {
                let mut branches_code = proc_macro2::TokenStream::new();
                for (i, branch) in branches.iter().enumerate() {
                    let mut branch_local_vars = local_vars.clone();
                    if let Some(target) = branch.cond.as_ref().and_then(|c| c.pattern.as_ref()) {
                        target_names(target, &mut branch_local_vars);
                    }
                    let body_tokens = generate_attr_string_code(&branch.parts, &branch_local_vars);
                    if let Some(ref cond) = branch.cond {
                        let cond_str = format_cond_test(cond, local_vars);
                        let cond_tokens: proc_macro2::TokenStream = cond_str.parse().unwrap();
                        if i == 0 {
                            branches_code.extend(quote! {
                                if #cond_tokens {
                                    s.push_str(&#body_tokens);
                                }
                            });
                        } else {
                            branches_code.extend(quote! {
                                else if #cond_tokens {
                                    s.push_str(&#body_tokens);
                                }
                            });
                        }
                    } else {
                        branches_code.extend(quote! {
                            else {
                                s.push_str(&#body_tokens);
                            }
                        });
                    }
                }
                code.push(quote! { &*({
                    let mut s = String::new();
                    #branches_code
                    s
                }) });
            }
        }
    }
    if code.is_empty() {
        quote! { "".to_string() }
    } else {
        quote! {
            [#(#code),*].join("")
        }
    }
}

/// The parts of `attr`'s value; a literal value is one part.
fn attr_parts(attr: &Attr) -> Vec<AttrPart> {
    match &attr.parts {
        Some(parts) => parts.clone(),
        None => vec![AttrPart::Lit(attr.value.clone())],
    }
}

// Selector matching against the template's elements, at compile time.
struct MacroElementWrapper<'t> {
    tpl: &'t Template,
    id: NodeId,
    active_classes: HashSet<String>,
    /// Conditional classes taken as set in this matching pass, by element;
    /// the others are taken as unset.
    assumed: std::rc::Rc<HashMap<NodeId, HashSet<String>>>,
    /// Whether a `checked="{{ ... }}"` is taken as checked in this
    /// matching pass (a literal `checked` speaks for itself).
    checked: bool,
    /// Stands for elements only known at runtime: a `{% for %}`/`{% if %}`
    /// wrapper, or what comes before or after a loop's or branch's body.
    opaque: bool,
    /// How many levels up from the element being styled this one is;
    /// `u32::MAX` off its chain of ancestors (a sibling).
    distance: u32,
    /// The `PointerLevel` (hover, active) assumed in this matching pass:
    /// elements at least that many levels up are hovered (pressed).
    pointer: (u32, u32),
}

impl<'t> MacroElementWrapper<'t> {
    fn with_assumed(tpl: &'t Template, id: NodeId, assumed: std::rc::Rc<HashMap<NodeId, HashSet<String>>>) -> Self {
        let opaque = is_template_element(tpl, id);
        let mut active_classes = get_static_classes(tpl, id);
        active_classes.extend(assumed.get(&id).into_iter().flatten().cloned());
        MacroElementWrapper {
            tpl,
            id,
            active_classes,
            assumed,
            checked: false,
            opaque,
            distance: u32::MAX,
            pointer: (u32::MAX, u32::MAX),
        }
    }

    /// The element `id` related to this one, `distance` levels up from
    /// the element being styled, in the same matching pass.
    fn relative(&self, id: NodeId, distance: u32) -> Self {
        MacroElementWrapper { distance, pointer: self.pointer, ..MacroElementWrapper::with_assumed(self.tpl, id, self.assumed.clone()) }
    }

    /// The element sibling `step` (-1 or 1) nodes away. Inside a loop's or
    /// branch's body, past its first or last element is whatever the
    /// template runs into, unknown until runtime.
    fn sibling(&self, step: isize) -> Option<Self> {
        let parent = self.tpl.node(self.id).parent?;
        let siblings = self.tpl.children(parent);
        let index = siblings.iter().position(|&child| child == self.id)?;
        let element = |&sibling: &NodeId| is_element(self.tpl, sibling).then(|| self.relative(sibling, u32::MAX));
        let found = if step < 0 {
            siblings[..index].iter().rev().find_map(element)
        } else {
            siblings[index + 1..].iter().find_map(element)
        };
        found.or_else(|| is_template_element(self.tpl, parent).then(|| self.relative(parent, u32::MAX)))
    }
}

/// Classes an element always has: literal names outside any `{% if %}`.
fn get_static_classes(tpl: &Template, id: NodeId) -> HashSet<String> {
    let mut static_classes = HashSet::new();
    if let Some(attr) = attribute(tpl, id, "class") {
        for (class, cond) in get_classes_and_conditions(attr, &HashSet::new()) {
            if cond.is_none() {
                static_classes.insert(class);
            }
        }
    }
    static_classes
}

thread_local! {
    /// The conditional classes of the elements compiled so far, by element:
    /// each class and the `bool` variable the element's code binds it to,
    /// in scope for its descendants' code.
    static CLASS_VARS: RefCell<HashMap<NodeId, Vec<(String, String)>>> = RefCell::new(HashMap::new());
}

/// Whether `id` is an element in the HTML sense: a tag, or a template
/// block (which html5ever saw as a `<template>` placeholder).
fn is_element(tpl: &Template, id: NodeId) -> bool {
    !matches!(tpl.node(id).kind, Kind::Document | Kind::Text(_))
}

/// A template block, or a `<template>` of the page: what stands for
/// elements only known at runtime.
fn is_template_element(tpl: &Template, id: NodeId) -> bool {
    match &tpl.node(id).kind {
        Kind::Element { tag, .. } => tag == "template",
        Kind::Expr(_) | Kind::For(..) | Kind::If | Kind::Branch(_) | Kind::Break | Kind::Continue => true,
        Kind::Document | Kind::Text(_) => false,
    }
}

impl xerune_css::selector::Element for MacroElementWrapper<'_> {
    // The nearest real element, skipping the template blocks. Siblings
    // still see the blocks, which are opaque: which iteration or branch
    // comes first is only known at runtime.
    fn parent(&self) -> Option<Self> {
        let mut parent = self.tpl.node(self.id).parent?;
        while is_template_element(self.tpl, parent) {
            parent = self.tpl.node(parent).parent?;
        }
        let distance = if self.distance == u32::MAX { u32::MAX } else { self.distance + 1 };
        matches!(self.tpl.node(parent).kind, Kind::Element { .. }).then(|| self.relative(parent, distance))
    }

    fn prev_sibling(&self) -> Option<Self> {
        self.sibling(-1)
    }

    fn next_sibling(&self) -> Option<Self> {
        self.sibling(1)
    }

    fn tag(&self) -> &str {
        match &self.tpl.node(self.id).kind {
            Kind::Element { tag, .. } => tag,
            _ if is_template_element(self.tpl, self.id) => "template",
            _ => "",
        }
    }

    fn attribute(&self, name: &str) -> Option<std::borrow::Cow<'_, str>> {
        let attr = attribute(self.tpl, self.id, name)?;
        Some(match name {
            // Only the classes active in this matching pass count. The raw
            // attribute is template source: `{% if on %}on{% endif %}`
            // contains the word `on` whether or not the class is applied.
            "class" => std::borrow::Cow::Owned(self.active_classes.iter().cloned().collect::<Vec<_>>().join(" ")),
            "checked" if attr.is_dynamic() => return self.checked.then(|| std::borrow::Cow::Borrowed("")),
            _ => std::borrow::Cow::Borrowed(attr.value.as_str()),
        })
    }

    fn has_class(&self, class: &str) -> bool {
        self.active_classes.contains(class)
    }

    fn is_empty(&self) -> bool {
        // A block's content is not its children in the HTML sense.
        is_template_element(self.tpl, self.id)
            || self.tpl.children(self.id).iter().all(|&child| match &self.tpl.node(child).kind {
                Kind::Text(text) => text.trim().is_empty(),
                _ => false,
            })
    }

    fn state(&self, state: xerune_css::selector::State) -> bool {
        use xerune_css::selector::State;
        // Hovered elements are one and its ancestors: an element on the
        // chain at or above the assumed level. Siblings are never taken as
        // hovered (`a:hover + b` doesn't match).
        let on_pointer = |level: u32| self.distance != u32::MAX && self.distance >= level;
        match state {
            // `checked="false"` is unchecked, as `parse_attributes_generic`
            // reads it.
            State::Checked => match attribute(self.tpl, self.id, "checked") {
                Some(attr) if attr.is_dynamic() => self.checked,
                Some(attr) => attr.value != "false",
                None => false,
            },
            State::Hover => on_pointer(self.pointer.0),
            State::Active => on_pointer(self.pointer.1),
            _ => false,
        }
    }

    fn is_opaque(&self) -> bool {
        self.opaque
    }
}

/// Whether `selector` matches `element`: `None` if it doesn't, else the
/// condition on the element's `__pointer` (its `PointerLevel`) under which
/// it does, `Some(None)` if it matches whatever the pointer does. Hovered
/// elements are one and its ancestors, so the levels to try are 0 (the
/// element itself) up to its root, and none.
fn pointer_match(selector: &xerune_css::selector::Selector, element: &mut MacroElementWrapper<'_>) -> Option<Option<String>> {
    use xerune_css::selector::State;
    let (hover, active) = (selector.uses_state(State::Hover), selector.uses_state(State::Active));
    element.pointer = (u32::MAX, u32::MAX);
    if !hover && !active {
        return selector.matches(element).then_some(None);
    }
    let depth = std::iter::successors(Some(element.id), |&id| element.tpl.node(id).parent).count() as u32;
    let levels: Vec<u32> = (0..=depth).chain([u32::MAX]).collect();
    let (hovers, actives) = match (hover, active) {
        (true, true) => (levels.clone(), levels),
        (true, false) => (levels, vec![u32::MAX]),
        _ => (vec![u32::MAX], levels),
    };
    let mut matching = Vec::new();
    for &h in &hovers {
        for &a in &actives {
            element.pointer = (h, a);
            if selector.matches(element) {
                matching.push((h, a));
            }
        }
    }
    element.pointer = (u32::MAX, u32::MAX);
    if matching.is_empty() {
        return None;
    }
    if matching.len() == hovers.len() * actives.len() {
        return Some(None);
    }
    // One of the levels matters: a test of it alone.
    let test = |field: &str, values: Vec<u32>| match values.iter().rposition(|&v| v != u32::MAX) {
        Some(last) if !values.contains(&u32::MAX) && values == (0..=values[last]).collect::<Vec<_>>() => {
            format!("__pointer.{field} <= {}", values[last])
        }
        _ => format!("matches!(__pointer.{field}, {})", values.iter().map(|v| format!("{v}u32")).collect::<Vec<_>>().join(" | ")),
    };
    Some(Some(match (hover, active) {
        (true, false) => test("hover", matching.iter().map(|m| m.0).collect()),
        (false, true) => test("active", matching.iter().map(|m| m.1).collect()),
        _ => matching.iter().map(|(h, a)| format!("(__pointer.hover == {h}u32 && __pointer.active == {a}u32)")).collect::<Vec<_>>().join(" || "),
    }))
}

/// `a && b` of two optional conditions.
fn and_condition(a: Option<String>, b: Option<String>) -> Option<String> {
    match (a, b) {
        (Some(a), Some(b)) => Some(format!("({a}) && ({b})")),
        (a, b) => a.or(b),
    }
}

/// Element `id`'s attribute `name`.
fn attribute<'t>(tpl: &'t Template, id: NodeId, name: &str) -> Option<&'t Attr> {
    match &tpl.node(id).kind {
        Kind::Element { attrs, .. } => attrs.iter().find(|a| a.name == name),
        _ => None,
    }
}

/// The classes of a `class` attribute, each with the condition under which
/// the element has it (`None`: always).
fn get_classes_and_conditions(
    class_attr: &Attr,
    local_vars: &HashSet<String>,
) -> Vec<(String, Option<String>)> {
    let Some(parts) = &class_attr.parts else {
        return class_attr.value.split_whitespace().map(|c| (c.to_string(), None)).collect();
    };

    fn walk_parts(
        parts: &[AttrPart],
        cond: Option<String>,
        local_vars: &HashSet<String>,
        res: &mut Vec<(String, Option<String>)>,
    ) {
        for part in parts {
            match part {
                AttrPart::Lit(val) => {
                    for cls in val.split_whitespace() {
                        if !cls.is_empty() {
                            res.push((cls.to_string(), cond.clone()));
                        }
                    }
                }
                AttrPart::If(branches) => {
                    // A branch applies when its own test holds and every
                    // earlier test in the chain failed (that is what makes
                    // `elif`/`else` exclusive).
                    let mut earlier: Vec<String> = Vec::new();
                    for branch in branches {
                        let mut parts: Vec<String> = cond.iter().cloned().collect();
                        parts.extend(earlier.iter().map(|c| format!("!({})", c)));
                        if let Some(ref c) = branch.cond {
                            let test = cond_test_as_bool(c, local_vars);
                            parts.push(test.clone());
                            earlier.push(test);
                        }
                        let branch_cond = match parts.len() {
                            0 => None,
                            1 => parts.pop(),
                            _ => Some(parts.iter().map(|p| format!("({})", p)).collect::<Vec<_>>().join(" && ")),
                        };
                        walk_parts(&branch.parts, branch_cond, local_vars, res);
                    }
                }
                // Rejected by `xerune_template`: classes must be literal.
                AttrPart::Expr(_) => {}
            }
        }
    }

    let mut res = Vec::new();
    walk_parts(parts, None, local_vars, &mut res);
    res
}

struct MatchedRule {
    /// The pseudo-element the declarations are for, if any.
    part: Option<xerune_css::ControlPart>,
    /// Name, value and whether it is `!important`.
    declarations: Vec<(String, String, bool)>,
    condition: Option<String>,
    /// Guard expression (`xerune::css::media::matches_query(...)`) when the
    /// rule was declared inside an `@media` block; None = always applies.
    media_guard: Option<proc_macro2::TokenStream>,
    /// From `@starting-style`: only for the style a new element starts from.
    starting: bool,
}

/// A style rule together with the `@media` guard it appeared under.
struct GuardedRule {
    rule: xerune_css::sheet::Rule,
    /// Some(expr) => wrap rule application in `if expr`; None => always.
    guard: Option<proc_macro2::TokenStream>,
}

/// Result of compiling an `@media` query text to a runtime guard.
enum MediaGuard {
    /// Query always matches (`all`, `screen`, or empty) — no guard emitted.
    Always,
    /// Query can match: a `matches_query` call.
    Guard(proc_macro2::TokenStream),
}

/// Compiles an `@media` query into a guard evaluated against
/// `xerune::screen`'s viewport when the view is built.
fn compile_media_guard(query: &xerune_css::media::Query) -> MediaGuard {
    use xerune_css::media::Feature;
    // A part with only a media type matches every viewport.
    if query.groups.iter().any(|g| !g.negated && g.features.is_empty()) {
        return MediaGuard::Always;
    }
    let groups = query.groups.iter().map(|group| {
        let negated = group.negated;
        let features = group.features.iter().map(|f| match *f {
            Feature::MinWidth(v) => quote! { ::xerune::__private::Feature::MinWidth(#v) },
            Feature::MaxWidth(v) => quote! { ::xerune::__private::Feature::MaxWidth(#v) },
            Feature::MinHeight(v) => quote! { ::xerune::__private::Feature::MinHeight(#v) },
            Feature::MaxHeight(v) => quote! { ::xerune::__private::Feature::MaxHeight(#v) },
            Feature::Landscape => quote! { ::xerune::__private::Feature::Landscape },
            Feature::Portrait => quote! { ::xerune::__private::Feature::Portrait },
        });
        quote! { ::xerune::__private::FeatureGroup { negated: #negated, features: &[#(#features),*] } }
    });
    MediaGuard::Guard(quote! { ::xerune::__private::matches_query(&[#(#groups),*]) })
}

fn compile_node(
    tpl: &Template,
    id: NodeId,
    rules: &[GuardedRule],
    local_vars: &mut HashSet<String>,
) -> proc_macro2::TokenStream {
    let children = tpl.children(id);
    match &tpl.node(id).kind {
        Kind::Document => {
            let children_code = compile_children(tpl, children, rules, local_vars);
            quote! {
                #(#children_code)*
            }
        }
        Kind::Expr(expr) => {
            let expr_str = format_expr(expr, local_vars);
            let expr_tokens: proc_macro2::TokenStream = expr_str.parse().unwrap();
            quote! {
                {
                    use ::xerune::__private::ToDisplayString;
                    let text_val = (#expr_tokens).to_display_string().into_owned();
                    builder.append_text_node(measurer, parent, &parent_style, &text_val);
                }
            }
        }
        Kind::For(pattern, iter) => {
            let var_str = format_target(pattern);
            let iter_str = match iter {
                // Ranges are iterators already; `(0..n).iter()` doesn't exist.
                Expr::Range(..) => format_expr(iter, local_vars),
                _ => format!("({}).iter()", format_expr(iter, local_vars)),
            };

            let mut loop_local_vars = local_vars.clone();
            target_names(pattern, &mut loop_local_vars);
            loop_local_vars.insert("_loop_item_index".to_string());
            loop_local_vars.insert("loop".to_string());

            let children_code = compile_children(tpl, children, rules, &mut loop_local_vars);

            let var_tokens: proc_macro2::TokenStream = var_str.parse().unwrap();
            let iter_tokens: proc_macro2::TokenStream = iter_str.parse().unwrap();

            let site = next_site();
            quote! {
                {
                    let parent = parent;
                    // A `data-key` element of the body is keyed
                    // by its key here instead of the index.
                    let __loop_key = ::xerune::__private::element_key(__parent_key, #site);
                    for (mut _loop_item_index, #var_tokens) in #iter_tokens.enumerate() {
                        let __parent_key = ::xerune::__private::element_key(__loop_key, _loop_item_index as u64);
                        #(#children_code)*
                    }
                }
            }
        }
        Kind::If => {
            let mut if_code = proc_macro2::TokenStream::new();
            for (branch_idx, &branch) in children.iter().enumerate() {
                let Kind::Branch(cond) = &tpl.node(branch).kind else { unreachable!() };
                // `{% if let Some(x) = ... %}` binds `x` inside this branch only.
                let mut branch_local_vars = local_vars.clone();
                if let Some(target) = cond.as_ref().and_then(|c| c.pattern.as_ref()) {
                    target_names(target, &mut branch_local_vars);
                }
                let children_code = compile_children(tpl, tpl.children(branch), rules, &mut branch_local_vars);

                if let Some(cond) = cond {
                    let cond_str = format_cond_test(cond, local_vars);
                    let cond_tokens: proc_macro2::TokenStream = cond_str.parse().unwrap();
                    if branch_idx == 0 {
                        if_code.extend(quote! {
                            if #cond_tokens {
                                #(#children_code)*
                            }
                        });
                    } else {
                        if_code.extend(quote! {
                            else if #cond_tokens {
                                #(#children_code)*
                            }
                        });
                    }
                } else {
                    if_code.extend(quote! {
                        else {
                            #(#children_code)*
                        }
                    });
                }
            }
            if_code
        }
        Kind::Branch(_) => unreachable!("branches are compiled by their `if`"),
        Kind::Break => quote! { break; },
        Kind::Continue => quote! { continue; },
        Kind::Element { tag, attrs } => {
            let tag = tag.as_str();
            if tag == "style" || tag == "script" || tag == "head" || tag == "template" {
                return quote! {};
            }

            // Standard Element styling logic
            let class_attr = attribute(tpl, id, "class");
            let mut static_classes = HashSet::new();
            // Each conditional class, with the test of whichever branches set it.
            let mut conditional_classes: Vec<(String, Vec<String>)> = Vec::new();

            if let Some(class_attr) = class_attr {
                let parsed_classes = get_classes_and_conditions(class_attr, local_vars);
                for (cls, cond_opt) in parsed_classes {
                    match cond_opt {
                        Some(cond) => match conditional_classes.iter_mut().find(|(c, _)| *c == cls) {
                            Some((_, conds)) => conds.push(cond),
                            None => conditional_classes.push((cls, vec![cond])),
                        },
                        None => {
                            static_classes.insert(cls);
                        }
                    }
                }
            }
            conditional_classes.retain(|(cls, _)| !static_classes.contains(cls));
            // Bound to variables here, so the rules of descendants, whose
            // code is nested in this element's, can test them too.
            let mut class_bindings = Vec::new();
            let mut class_vars = Vec::new();
            for (cls, conds) in &conditional_classes {
                let var = format!("__class_{}", next_site());
                let ident = syn::Ident::new(&var, proc_macro2::Span::call_site());
                let test: proc_macro2::TokenStream = conds.iter().map(|c| format!("({c})")).collect::<Vec<_>>().join(" || ").parse().unwrap();
                class_bindings.push(quote! { let #ident: bool = #test; });
                class_vars.push((cls.clone(), var));
            }
            CLASS_VARS.with(|m| m.borrow_mut().insert(id, class_vars));
            // The conditional classes of the element and its ancestors.
            let mut chain_classes: Vec<(NodeId, String, String)> = Vec::new();
            let mut ancestor = Some(id);
            while let Some(element) = ancestor {
                CLASS_VARS.with(|m| {
                    for (cls, var) in m.borrow().get(&element).into_iter().flatten() {
                        chain_classes.push((element, cls.clone(), var.clone()));
                    }
                });
                ancestor = tpl.node(element).parent;
            }

            // A `checked="{{ ... }}"` (what `{% if c %}checked{% endif %}`
            // becomes) is checked at runtime unless it is "false", like
            // `parse_attributes_generic` reads it; `:checked` rules apply
            // under that condition.
            let dynamic_checked = attrs.iter().position(|a| a.name == "checked" && a.is_dynamic());

            let has_parts = matches!(tag, "input" | "progress");
            let mut matched_rules = Vec::new();
            for GuardedRule { rule, guard } in rules {
                // Only form controls have parts (`*::-moz-range-thumb`).
                let part = match rule.selector.pseudo_element() {
                    None => None,
                    Some(PseudoElement::Part(part)) if has_parts || !part.of_form_control() => Some(part),
                    Some(_) => continue,
                };
                // What is only known at runtime and the rule tests: the
                // conditional classes it names, and a dynamic `checked`.
                let classes: Vec<&(NodeId, String, String)> = chain_classes.iter().filter(|(_, cls, _)| rule.selector.mentions_class(cls)).take(10).collect();
                let checked = dynamic_checked.is_some() && rule.selector.uses_state(xerune_css::selector::State::Checked);
                let unknowns = classes.len() + checked as usize;
                // Whether it matches (and under which pointer condition)
                // with each combination of them set.
                let outcomes: Vec<Option<Option<String>>> = (0..1usize << unknowns).map(|set| {
                    let mut assumed: HashMap<NodeId, HashSet<String>> = HashMap::new();
                    for (i, (id, cls, _)) in classes.iter().enumerate() {
                        if set & (1 << i) != 0 {
                            assumed.entry(*id).or_default().insert(cls.clone());
                        }
                    }
                    let mut wrapper = MacroElementWrapper::with_assumed(tpl, id, std::rc::Rc::new(assumed));
                    wrapper.distance = 0;
                    wrapper.checked = checked && set & (1 << classes.len()) != 0;
                    pointer_match(&rule.selector, &mut wrapper)
                }).collect();
                // Leave out the unknowns the outcome doesn't depend on.
                let relevant: Vec<usize> = (0..unknowns).filter(|&i| (0..outcomes.len()).any(|set| outcomes[set] != outcomes[set ^ (1 << i)])).collect();
                let tests: Vec<String> = classes.iter().map(|(_, _, var)| var.clone()).chain(checked.then(|| "_dyn_checked".to_string())).collect();
                let mut conditions: Vec<(Option<String>, Vec<String>)> = Vec::new();
                let irrelevant: usize = (0..unknowns).filter(|i| !relevant.contains(i)).map(|i| 1 << i).sum();
                // One combination per assignment of the relevant ones.
                for (set, outcome) in outcomes.iter().enumerate().filter(|(set, _)| set & irrelevant == 0) {
                    let Some(pointer) = outcome else { continue };
                    let literals: Vec<String> = relevant.iter().map(|&i| if set & (1 << i) != 0 { tests[i].clone() } else { format!("!{}", tests[i]) }).collect();
                    match conditions.iter_mut().find(|(p, _)| p == pointer) {
                        Some((_, alternatives)) => alternatives.push(literals.join(" && ")),
                        None => conditions.push((pointer.clone(), vec![literals.join(" && ")])),
                    }
                }
                let declarations: Vec<(String, String, bool)> = rule.declarations.iter()
                    .map(|d| (d.name.clone(), d.value.clone(), d.important))
                    .collect();
                for (pointer, alternatives) in conditions {
                    let known = (!relevant.is_empty()).then(|| alternatives.iter().map(|a| format!("({a})")).collect::<Vec<_>>().join(" || "));
                    matched_rules.push(MatchedRule {
                        part,
                        declarations: declarations.clone(),
                        condition: and_condition(known, pointer),
                        media_guard: guard.clone(),
                        starting: rule.starting,
                    });
                }
            }

            let mut rule_applications = Vec::new();
            let mut important_applications = Vec::new();
            // With the `@starting-style` rules too, in cascade order: the
            // style a new element starts from.
            let has_starting = matched_rules.iter().any(|r| r.starting);
            let mut starting_applications = Vec::new();
            let mut starting_important_applications = Vec::new();
            for rule in matched_rules {
                let important = rule.part.is_none();
                let (important_declarations, declarations): (Vec<_>, Vec<_>) = rule.declarations.into_iter().partition(|d| important && d.2);
                for (declarations, applications, starting_applications) in [(declarations, &mut rule_applications, &mut starting_applications), (important_declarations, &mut important_applications, &mut starting_important_applications)] {
                    if declarations.is_empty() {
                        continue;
                    }
                    let (names, values): (Vec<_>, Vec<_>) = declarations.into_iter().map(|(name, value, _)| (name, value)).unzip();
                    let mut body = match rule.part {
                        None => quote! { el.apply(&[#((#names, #values)),*]); },
                        Some(part) => {
                            let part = syn::Ident::new(&format!("{part:?}"), proc_macro2::Span::call_site());
                            quote! { el.apply_part(::xerune::__private::ControlPart::#part, &[#((#names, #values)),*]); }
                        }
                    };
                    if let Some(cond) = &rule.condition {
                        let cond_tokens: proc_macro2::TokenStream = cond.parse().unwrap();
                        body = quote! {
                            if #cond_tokens {
                                #body
                            }
                        };
                    }
                    if let Some(guard) = &rule.media_guard {
                        body = quote! {
                            if #guard {
                                #body
                            }
                        };
                    }
                    if has_starting {
                        starting_applications.push(body.clone());
                    }
                    if !rule.starting {
                        applications.push(body);
                    }
                }
            }
            let style_attribute = attribute(tpl, id, "style");
            if style_attribute.is_some_and(|style| style.value.contains('!') || style.is_dynamic()) {
                important_applications.push(quote! { el.apply_important_style(attrs_slice); });
                starting_important_applications.push(quote! { el.apply_important_style(attrs_slice); });
            }
            let (starting_capture, starting_finish) = if has_starting {
                (
                    quote! { let __starting = builder.is_new(__key).then(|| el.clone()); },
                    quote! {
                        if let Some(mut el) = __starting {
                            #(#starting_applications)*
                            builder.starting_attributes(&mut el, attrs_slice, message_validator);
                            #(#starting_important_applications)*
                            builder.set_starting_style(node, el, &parent_style);
                        }
                    },
                )
            } else {
                (quote! {}, quote! {})
            };

            // Create attribute constructor
            let mut static_attrs = Vec::new();
            let mut dynamic_attrs = Vec::new();
            let mut dynamic_vars = Vec::new();
            let mut click = quote! {};
            for (idx, attr) in attrs.iter().enumerate() {
                let key = attr.name.as_str();
                let val = attr.value.as_str();
                if let Some(method) = message_method(key) {
                    let handler = compile_message(attr, method, local_vars);
                    click.extend(handler);
                    continue;
                }
                if attr.is_dynamic() {
                    let val_tokens = generate_attr_string_code(&attr_parts(attr), local_vars);
                    let var_name = syn::Ident::new(&format!("_dyn_attr_{}", idx), proc_macro2::Span::call_site());
                    dynamic_vars.push(quote! {
                        let #var_name: String = #val_tokens;
                    });
                    dynamic_attrs.push(quote! { (::xerune::__private::Cow::Borrowed(#key), ::xerune::__private::Cow::Owned(#var_name)) });
                } else {
                    static_attrs.push(quote! { (::xerune::__private::Cow::Borrowed(#key), ::xerune::__private::Cow::Borrowed(#val)) });
                }
            }

            if let Some(idx) = dynamic_checked {
                let var_name = syn::Ident::new(&format!("_dyn_attr_{}", idx), proc_macro2::Span::call_site());
                dynamic_vars.push(quote! {
                    let _dyn_checked: bool = #var_name != "false";
                });
            }

            // A literal input `type` picks the widget defaults up front, so
            // the CSS rules below apply on top of them.
            let static_input_type = attrs.iter()
                .find(|a| tag == "input" && a.name == "type" && !a.is_dynamic())
                .map(|a| a.value.clone());
            let element_style = match static_input_type {
                Some(input_type) => quote! { ::xerune::__private::ElementStyle::new_input(#input_type, &parent_style) },
                None => quote! { ::xerune::__private::ElementStyle::new(#tag, &parent_style) },
            };

            let child_compilation = compile_children(tpl, children, rules, local_vars);

            let site = next_site();
            // `data-key` names the element among its siblings, so it keeps
            // its key when items before it come and go.
            let key = match attribute(tpl, id, "data-key") {
                Some(attr) => {
                    let text = generate_attr_string_code(&attr_parts(attr), local_vars);
                    quote! {
                        ::xerune::__private::element_key(
                            ::xerune::__private::element_key(__loop_key, ::xerune::model::hash_bytes(#text.as_bytes())),
                            #site,
                        )
                    }
                }
                None => quote! { ::xerune::__private::element_key(__parent_key, #site) },
            };
            quote! {
                #(#class_bindings)*
                let __key = #key;
                let __pointer = builder.pointer_level(__key, __parent_pointer);
                let (el, node) = {
                    #(#dynamic_vars)*
                    let attrs_slice: &[(::xerune::__private::Cow<'static, str>, ::xerune::__private::Cow<'_, str>)] = &[
                        #(#static_attrs,)*
                        #(#dynamic_attrs),*
                    ];
                    let mut el = #element_style;
                    #starting_capture
                    // Match and apply CSS rules
                    #(#rule_applications)*
                    #click
                    let node = builder.begin_element(&mut el, parent, attrs_slice, message_validator, __key);
                    #(#important_applications)*
                    #starting_finish
                    (el, node)
                };
                {
                    let parent = node;
                    let parent_style = &el.style;
                    let __parent_key = __key;
                    let __loop_key = __key;
                    let __parent_pointer = __pointer;
                    #(#child_compilation)*
                }
                builder.finish_element(el, node, &parent_style);
            }
        }
        Kind::Text(text) => {
            if text.trim().is_empty() {
                return quote! {};
            }
            quote! {
                builder.append_text_node(measurer, parent, &parent_style, #text);
            }
        }
    }
}

/// The `ElementStyle` method that sets an element's message attribute
/// (`data-on-click` and the hover ones).
fn message_method(attribute: &str) -> Option<&'static str> {
    match attribute {
        "data-on-click" => Some("on_click"),
        "data-on-hover" => Some("on_hover"),
        "data-on-hover-out" => Some("on_hover_out"),
        "data-on-transitionrun" => Some("on_transitionrun"),
        "data-on-transitionstart" => Some("on_transitionstart"),
        "data-on-transitionend" => Some("on_transitionend"),
        "data-on-transitioncancel" => Some("on_transitioncancel"),
        _ => None,
    }
}

/// The message a `data-on-click` (or hover) attribute sends. `prefix:{{ expr }}` calls the
/// variant's constructor generated by `#[derive(XeruneMessage)]`, so the
/// argument's type is checked by the compiler. Anything else (`add`,
/// `digit:7`, text around the expression) is parsed with `FromStr` when the
/// view is built, after the name was checked at compile time.
fn compile_message(attr: &Attr, method: &str, local_vars: &HashSet<String>) -> proc_macro2::TokenStream {
    let set = syn::Ident::new(method, proc_macro2::Span::call_site());
    let set_text = syn::Ident::new(&format!("{method}_text"), proc_macro2::Span::call_site());
    let message_type = quote! { <Self as ::xerune::Model>::Message };
    let val = attr.value.as_str();
    let Some(parts) = attr.parts.as_deref().filter(|_| attr.is_dynamic()) else {
        return quote! { el.#set_text::<#message_type>(#val); };
    };
    if let [AttrPart::Lit(prefix), AttrPart::Expr(expr)] = parts {
        if !prefix.is_empty() && prefix.trim() == prefix {
            let constructor = message_constructor(prefix);
            let expr: proc_macro2::TokenStream = format_expr(expr, local_vars).parse().unwrap();
            return quote! {
                el.#set(#message_type::#constructor(::xerune::__private::message_arg(&(#expr))));
            };
        }
    }
    let text = generate_attr_string_code(parts, local_vars);
    quote! { el.#set_text::<#message_type>(&#text); }
}

/// The `@keyframes` blocks of `css`, with comments removed.
fn extract_keyframes(css: &str) -> String {
    let mut stripped = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        stripped.push_str(&rest[..start]);
        rest = rest[start + 2..].find("*/").map_or("", |end| &rest[start + 2 + end + 2..]);
    }
    stripped.push_str(rest);

    let mut out = String::new();
    let mut rest = stripped.as_str();
    while let Some(start) = rest.find("@keyframes") {
        let block = &rest[start..];
        let Some(open) = block.find('{') else { break };
        let mut depth = 0;
        let mut end = block.len();
        for (i, c) in block[open..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = open + i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        out.push_str(&block[..end]);
        out.push('\n');
        rest = &block[end..];
    }
    out
}

// Helper to find a template file dynamically in parent dir, example templates dirs, root templates dir, or manifest dir
/// Finds a template, trying in order: next to the including template (for
/// `{% include %}`), the crate root, the crate's `templates/` directory, and
/// the workspace root (so crates of a workspace can share templates).
/// Returns its full path and a short name for messages.
fn find_template_file(
    path_str: &str,
    cargo_manifest_dir: &std::path::Path,
    parent_dir: Option<&std::path::Path>,
) -> Result<(std::path::PathBuf, String), String> {
    let workspace = workspace_root(cargo_manifest_dir);
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    candidates.extend(parent_dir.map(|dir| dir.join(path_str)));
    candidates.push(cargo_manifest_dir.join(path_str));
    candidates.push(cargo_manifest_dir.join("templates").join(path_str));
    candidates.extend(workspace.as_deref().filter(|ws| *ws != cargo_manifest_dir).map(|ws| ws.join(path_str)));

    let Some(found) = candidates.iter().find(|c| c.is_file()) else {
        let tried: Vec<String> = candidates.iter().map(|c| format!("  {}", c.display())).collect();
        return Err(format!("template `{path_str}` not found; looked at:\n{}", tried.join("\n")));
    };
    let short = [Some(cargo_manifest_dir), workspace.as_deref()]
        .into_iter()
        .flatten()
        .find_map(|root| found.strip_prefix(root).ok())
        .unwrap_or(found)
        .to_string_lossy()
        .replace('\\', "/");
    Ok((found.clone(), short))
}

/// The nearest directory at or above `dir` whose `Cargo.toml` declares a
/// `[workspace]`.
fn workspace_root(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    dir.ancestors()
        .find(|d| {
            std::fs::read_to_string(d.join("Cargo.toml"))
                .is_ok_and(|toml| toml.lines().any(|line| line.trim() == "[workspace]"))
        })
        .map(std::path::Path::to_path_buf)
}

/// Message for panics that are not `TemplateError`s, i.e. compiler bugs.
fn internal_error_message(payload: &(dyn std::any::Any + Send)) -> String {
    let detail = payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .unwrap_or("unknown panic");
    format!("internal error in the xerune template compiler (please report this, with the template): {detail}")
}

#[proc_macro_derive(XeruneTemplate, attributes(template))]
pub fn derive_xerune_template(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    // Find the template from attributes: a file (`path`) or inline (`source`)
    let mut template = None;
    for attr in &input.attrs {
        if attr.path().is_ident("template") {
            let parsed = attr.parse_nested_meta(|meta| {
                let kind = if meta.path.is_ident("path") {
                    TemplateRef::Path
                } else if meta.path.is_ident("source") {
                    TemplateRef::Source
                } else {
                    return Err(meta.error("unsupported attribute; expected `path = \"...\"` or `source = \"...\"`"));
                };
                let lit: syn::LitStr = meta.value()?.parse()?;
                if template.is_some() {
                    return Err(meta.error("only one of `path` / `source` may be given"));
                }
                template = Some((kind(lit.value()), lit.span()));
                Ok(())
            });
            if let Err(e) = parsed {
                return e.to_compile_error().into();
            }
        }
    }
    let Some((template, template_span)) = template else {
        return syn::Error::new_spanned(&input.ident, "XeruneTemplate requires a `#[template(path = \"...\")]` attribute")
            .to_compile_error()
            .into();
    };

    // Template errors unwind out of the (deeply recursive) compiler as
    // `TemplateError` panics; report them as ordinary compile errors pointing
    // at the `path = "..."` literal. Silence the default hook meanwhile so the
    // build output isn't cluttered with "thread panicked" noise.
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| expand_template(&input, &template)));
    std::panic::set_hook(prev_hook);
    CLASS_VARS.with(|m| m.borrow_mut().clear());

    match result {
        Ok(tokens) => tokens.into(),
        Err(payload) => {
            let msg = match payload.downcast::<TemplateError>() {
                Ok(err) => err.0,
                Err(other) => internal_error_message(&*other),
            };
            syn::Error::new(template_span, msg).to_compile_error().into()
        }
    }
}

/// `#[template(path = "...")]` or `#[template(source = "...")]`.
enum TemplateRef {
    Path(String),
    Source(String),
}

fn expand_template(input: &DeriveInput, template: &TemplateRef) -> proc_macro2::TokenStream {
    let name = &input.ident;

    let cargo_manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let cargo_manifest_path = PathBuf::from(&cargo_manifest_dir);

    // Load the template; includes in an inline template resolve from the manifest dir
    let mut tracked_paths = Vec::new();
    let root = match template {
        TemplateRef::Path(template_path) => {
            let (full_template_path, short_path) = find_template_file(template_path, &cargo_manifest_path, None)
                .unwrap_or_else(|e| fail(e));
            let template_content = std::fs::read_to_string(&full_template_path)
                .unwrap_or_else(|e| fail(format_args!("failed to read template file {full_template_path:?}: {e}")));
            tracked_paths.push(full_template_path.to_string_lossy().into_owned());
            xerune_template::File {
                name: short_path,
                dir: full_template_path.parent().unwrap_or(&cargo_manifest_path).to_path_buf(),
                content: template_content,
            }
        }
        TemplateRef::Source(source) => xerune_template::File {
            name: "<inline template>".to_string(),
            dir: cargo_manifest_path.clone(),
            content: source.clone(),
        },
    };
    let tpl = xerune_template::parse(root, &mut |path: &str, dir: &std::path::Path| {
        let (full_path, short_path) = find_template_file(path, &cargo_manifest_path, Some(dir))?;
        let content = std::fs::read_to_string(&full_path)
            .map_err(|e| format!("failed to read included template {full_path:?}: {e}"))?;
        tracked_paths.push(full_path.to_string_lossy().into_owned());
        Ok(xerune_template::File { name: short_path, dir: full_path.parent().unwrap_or(dir).to_path_buf(), content })
    })
    .unwrap_or_else(|e| fail(e));
    let css_content = &tpl.css;

    // Rebuild when a template changes: cargo tracks files read by include_bytes.
    tracked_paths.sort();
    tracked_paths.dedup();
    let dummy_includes = tracked_paths.iter().map(|path| {
        quote! {
            const _: &[u8] = include_bytes!(#path);
        }
    });

    NEXT_SITE.with(|site| site.set(0));

    // Each rule of an `@media` block applies under a runtime viewport guard
    // of its block's queries (all of them, for nested blocks).
    let sheet = xerune_css::sheet::StyleSheet::parse(css_content);
    for selector in &sheet.dropped {
        eprintln!("xerune: CSS rule '{}' is ignored: its selector is invalid or uses something unsupported", selector);
    }
    let guards: Vec<Option<proc_macro2::TokenStream>> = sheet.media.iter().map(|queries| {
        let guards: Vec<_> = queries.iter().filter_map(|query| {
            if query.unsupported {
                eprintln!("xerune: @media query '{}' uses unsupported/unknown features; those parts never apply", query.text);
            }
            match compile_media_guard(query) {
                MediaGuard::Always => None,
                MediaGuard::Guard(guard) => Some(guard),
            }
        }).collect();
        (!guards.is_empty()).then(|| quote! { #(#guards)&&* })
    }).collect();
    let rules: Vec<GuardedRule> = sheet.rules.into_iter()
        .map(|rule| GuardedRule { guard: rule.media.and_then(|m| guards[m].clone()), rule })
        .collect();
    let keyframes_css = css_content.clone();

    // Collect data-on-click action strings
    let mut raw_actions = Vec::new();
    // Not inside template blocks, whose content html5ever kept apart.
    fn collect_actions(tpl: &Template, id: NodeId, actions: &mut Vec<String>) {
        if let Kind::Element { attrs, .. } = &tpl.node(id).kind {
            for attr in attrs {
                if message_method(&attr.name).is_some() {
                    actions.push(attr.value.clone());
                }
            }
        }
        if is_template_element(tpl, id) {
            return;
        }
        for &child in tpl.children(id) {
            collect_actions(tpl, child, actions);
        }
    }
    collect_actions(&tpl, Template::ROOT, &mut raw_actions);

    let mut check_calls = Vec::new();
    for action in raw_actions {
        if let Some(idx) = action.find("{{").or_else(|| action.find("{%")) {
            let prefix = &action[..idx];
            if !prefix.is_empty() {
                let panic_msg = format!("Invalid action prefix '{}' in HTML template", prefix);
                check_calls.push(quote! {
                    {
                        let mut found = false;
                        let mut i = 0;
                        while i < prefixes.len() {
                            if const_str_starts_with(#prefix, prefixes[i]) {
                                found = true;
                                break;
                            }
                            i += 1;
                        }
                        if !found {
                            panic!(#panic_msg);
                        }
                    }
                });
            }
        } else {
            let panic_msg = format!("Invalid action '{}' in HTML template", action);
            check_calls.push(quote! {
                {
                    let mut found = false;
                    let mut i = 0;
                    while i < exact.len() {
                        if const_str_eq(#action, exact[i]) {
                            found = true;
                            break;
                        }
                        i += 1;
                    }
                    if !found {
                        i = 0;
                        while i < prefixes.len() {
                            if const_str_starts_with(#action, prefixes[i]) {
                                found = true;
                                break;
                            }
                            i += 1;
                        }
                    }
                    if !found {
                        panic!(#panic_msg);
                    }
                }
            });
        }
    }

    // Only the @keyframes blocks are parsed at runtime (on every rebuild),
    // so hand over just those, and nothing for templates without any.
    let keyframes_css = extract_keyframes(&keyframes_css);
    let keyframes_init = if keyframes_css.is_empty() {
        quote! {}
    } else {
        quote! { builder.set_keyframes(::xerune::__private::parse_keyframes(#keyframes_css)); }
    };

    let starting_init = if rules.iter().any(|r| r.rule.starting) {
        quote! { builder.note_starting_style(); }
    } else {
        quote! {}
    };
    let mut local_vars = HashSet::new();
    let uses = |state| rules.iter().any(|r| r.rule.selector.uses_state(state));
    let (uses_hover, uses_active) = (uses(xerune_css::selector::State::Hover), uses(xerune_css::selector::State::Active));
    let body_compilation = compile_node(&tpl, Template::ROOT, &rules, &mut local_vars);

    let method_table = method_table(name, &tpl);
    let expanded = quote! {
        #(#dummy_includes)*

        impl ::xerune::__private::TemplateLayout for #name {
            fn stylesheet(&self) -> &'static str {
                ""
            }

            fn uses_pointer(&self) -> (bool, bool) {
                (#uses_hover, #uses_active)
            }

            fn build_ui(
                &self,
                builder: &mut ::xerune::__private::UiBuilder,
                measurer: &impl ::xerune::TextMeasurer,
                default_style: &::xerune::__private::ContainerStyle,
                message_validator: &impl Fn(&str) -> bool,
            ) -> ::xerune::__private::NodeId {
                const _: () = {
                    const fn const_str_eq(a: &str, b: &str) -> bool {
                        let a_bytes = a.as_bytes();
                        let b_bytes = b.as_bytes();
                        if a_bytes.len() != b_bytes.len() { return false; }
                        let mut i = 0;
                        while i < a_bytes.len() {
                            if a_bytes[i] != b_bytes[i] { return false; }
                            i += 1;
                        }
                        true
                    }
                    const fn const_str_starts_with(s: &str, prefix: &str) -> bool {
                        let s_bytes = s.as_bytes();
                        let p_bytes = prefix.as_bytes();
                        if s_bytes.len() < p_bytes.len() { return false; }
                        let mut i = 0;
                        while i < p_bytes.len() {
                            if s_bytes[i] != p_bytes[i] { return false; }
                            i += 1;
                        }
                        true
                    }

                    type MsgType = <#name as ::xerune::Model>::Message;
                    let exact = <MsgType as ::xerune::XeruneMessage>::VALID_EXACT;
                    let prefixes = <MsgType as ::xerune::XeruneMessage>::VALID_PREFIXES;

                    #(#check_calls)*
                };

                #keyframes_init
                #starting_init
                let parent = builder.begin_root(default_style);
                let parent_style = default_style.clone();
                let __parent_key: u64 = 0;
                let __loop_key: u64 = 0;
                let __parent_pointer = ::xerune::__private::PointerLevel::NONE;
                {
                    let parent = parent;
                    let parent_style = parent_style;
                    #body_compilation
                }
                parent
            }
        }

        #method_table
    };

    expanded
}

/// The model's methods the template calls, by `Expr::method_key`, with
/// their number of arguments.
fn template_methods(tpl: &Template) -> Vec<(String, usize)> {
    fn expr(e: &Expr, locals: &[String], out: &mut Vec<(String, usize)>) {
        let mut sub = |e: &Expr| expr(e, locals, out);
        match e {
            Expr::Call(callee, args) => {
                if let Some(key) = Expr::method_key(callee, &|name| locals.iter().any(|l| l == name)) {
                    if !out.iter().any(|(k, _)| *k == key) {
                        out.push((key, args.len()));
                    }
                }
                expr(callee, locals, out);
                args.iter().for_each(|a| expr(a, locals, out));
            }
            Expr::Array(items) | Expr::Tuple(items) | Expr::Format(items) => items.iter().for_each(sub),
            Expr::Attr(obj, _) | Expr::Unary(_, obj) | Expr::Group(obj) => sub(obj),
            Expr::Index(a, b) | Expr::Binary(_, a, b) => {
                sub(a);
                sub(b);
            }
            Expr::Range(_, a, b) => a.iter().chain(b).for_each(|e| sub(e)),
            Expr::Bool(_) | Expr::Num(_) | Expr::Str(_) | Expr::Char(_) | Expr::Var(_) | Expr::Path(_) => {}
        }
    }
    fn bound(cond: &Option<Cond>, locals: &[String]) -> Vec<String> {
        let mut locals = locals.to_vec();
        if let Some(pattern) = cond.as_ref().and_then(|c| c.pattern.as_ref()) {
            pattern.names(&mut locals);
        }
        locals
    }
    fn attr_values(parts: &[AttrPart], locals: &[String], out: &mut Vec<(String, usize)>) {
        for part in parts {
            match part {
                AttrPart::Lit(_) => {}
                AttrPart::Expr(e) => expr(e, locals, out),
                AttrPart::If(branches) => {
                    for branch in branches {
                        if let Some(cond) = &branch.cond {
                            expr(&cond.expr, locals, out);
                        }
                        attr_values(&branch.parts, &bound(&branch.cond, locals), out);
                    }
                }
            }
        }
    }
    fn node(tpl: &Template, id: NodeId, locals: &[String], out: &mut Vec<(String, usize)>) {
        let mut locals = locals.to_vec();
        match &tpl.node(id).kind {
            Kind::Expr(e) => expr(e, &locals, out),
            Kind::For(pattern, iter) => {
                expr(iter, &locals, out);
                pattern.names(&mut locals);
                locals.push("loop".to_string());
            }
            Kind::Branch(cond) => {
                if let Some(cond) = cond {
                    expr(&cond.expr, &locals, out);
                }
                locals = bound(cond, &locals);
            }
            Kind::Element { attrs, .. } => {
                for attr in attrs {
                    attr_values(&attr_parts(attr), &locals, out);
                }
            }
            _ => {}
        }
        for &child in tpl.children(id) {
            node(tpl, child, &locals, out);
        }
    }
    let mut out = Vec::new();
    node(tpl, Template::ROOT, &[], &mut out);
    out
}

/// The studio's table of the model's methods the template calls
/// (`xerune_template::Methods`), when xerune's `interpret` feature is on.
fn method_table(name: &syn::Ident, tpl: &Template) -> proc_macro2::TokenStream {
    let arms = template_methods(tpl).into_iter().map(|(key, arity)| {
        let call: proc_macro2::TokenStream = format!("self.{key}").parse().unwrap();
        let args = (0..arity).map(|i| quote! { ::xerune::__private::FromValue::from_value(arg(#i)?)? });
        quote! {
            #key => Some((|| ::xerune::__private::to_value(&#call(#(#args),*)))()),
        }
    });
    quote! {
        ::xerune::__private::if_interpret! {
            impl ::xerune::__private::TemplateMethods for #name {
                #[allow(unused_variables)]
                fn call(&self, key: &str, args: &[::xerune::__private::Value]) -> Option<Result<::xerune::__private::Value, String>> {
                    let arg = |i: usize| args.get(i).ok_or_else(|| format!("`{key}` needs more arguments"));
                    match key {
                        #(#arms)*
                        _ => None,
                    }
                }
            }
        }
    }
}

/// The hidden constructor `#[derive(XeruneMessage)]` generates for a message
/// name (`add`) or prefix (`toggle:`), which compiled templates call. `_` is
/// doubled and other non-alphanumerics are hex-escaped, so distinct messages
/// never share a name.
fn message_constructor(message: &str) -> syn::Ident {
    let mut name = String::from("__xerune_msg_");
    for c in message.chars() {
        match c {
            c if c.is_ascii_alphanumeric() => name.push(c),
            '_' => name.push_str("__"),
            c => name.push_str(&format!("_{:x}_", c as u32)),
        }
    }
    syn::Ident::new(&name, proc_macro2::Span::call_site())
}

#[proc_macro_derive(XeruneMessage, attributes(xerune))]
pub fn derive_xerune_message(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    let mut no_from_str = false;
    for attr in &input.attrs {
        if attr.path().is_ident("xerune") {
            let _ = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("no_from_str") {
                    no_from_str = true;
                    Ok(())
                } else {
                    Err(meta.error("unsupported attribute"))
                }
            });
        }
    }

    let data = match &input.data {
        syn::Data::Enum(d) => d,
        _ => panic!("XeruneMessage can only be derived on enums"),
    };

    let mut exact_matches = Vec::new();
    let mut prefix_matches = Vec::new();
    let mut from_str_branches = Vec::new();
    let mut constructors = Vec::new();

    for variant in &data.variants {
        let variant_name = &variant.ident;

        let to_snake_case = |s: &str| {
            let mut res = String::new();
            for (i, c) in s.chars().enumerate() {
                if c.is_uppercase() {
                    if i > 0 {
                        res.push('_');
                    }
                    res.push(c.to_ascii_lowercase());
                } else {
                    res.push(c);
                }
            }
            res
        };

        let mut custom_rename = None;
        let mut custom_prefix = None;
        for attr in &variant.attrs {
            if attr.path().is_ident("xerune") {
                let _ = attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("rename") {
                        let value = meta.value()?;
                        let s: syn::LitStr = value.parse()?;
                        custom_rename = Some(s.value());
                        Ok(())
                    } else if meta.path.is_ident("prefix") {
                        let value = meta.value()?;
                        let s: syn::LitStr = value.parse()?;
                        custom_prefix = Some(s.value());
                        Ok(())
                    } else {
                        Err(meta.error("unsupported attribute"))
                    }
                });
            }
        }

        match &variant.fields {
            syn::Fields::Unit => {
                let exact_str = custom_rename.unwrap_or_else(|| to_snake_case(&variant_name.to_string()));
                exact_matches.push(exact_str.clone());
                from_str_branches.push(quote! {
                    #exact_str => Ok(#name::#variant_name),
                });
                let constructor = message_constructor(&exact_str);
                constructors.push(quote! {
                    pub fn #constructor() -> Self {
                        #name::#variant_name
                    }
                });
            }
            syn::Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                let field = &fields.unnamed[0];
                let field_type = &field.ty;

                let prefix_str = custom_prefix.unwrap_or_else(|| {
                    let mut s = to_snake_case(&variant_name.to_string());
                    s.push(':');
                    s
                });
                prefix_matches.push(prefix_str.clone());
                let constructor = message_constructor(&prefix_str);
                constructors.push(quote! {
                    pub fn #constructor(value: #field_type) -> Self {
                        #name::#variant_name(value)
                    }
                });

                from_str_branches.push(quote! {
                    _s if _s.starts_with(#prefix_str) => {
                        let payload = &_s[#prefix_str.len()..];
                        if let Ok(val) = payload.parse::<#field_type>() {
                            Ok(#name::#variant_name(val))
                        } else {
                            Err(())
                        }
                    }
                });
            }
            _ => panic!("XeruneMessage only supports unit variants and single-field tuple variants currently. For custom variants, please use manual implementation with #[xerune(no_from_str)]."),
        }
    }

    let from_str_impl = if no_from_str {
        quote! {}
    } else {
        quote! {
            impl core::str::FromStr for #name {
                type Err = ();
                fn from_str(s: &str) -> Result<Self, Self::Err> {
                    match s {
                        #(#from_str_branches)*
                        _ => Err(()),
                    }
                }
            }
        }
    };

    let expanded = quote! {
        #from_str_impl

        /// Constructors for compiled `data-on-click` messages.
        #[doc(hidden)]
        #[allow(non_snake_case)]
        impl #name {
            #(#constructors)*
        }

        impl ::xerune::XeruneMessage for #name {
            const VALID_PREFIXES: &'static [&'static str] = &[#(#prefix_matches),*];
            const VALID_EXACT: &'static [&'static str] = &[#(#exact_matches),*];
        }
    };

    TokenStream::from(expanded)
}
