//! The parsed template: HTML elements and template blocks in one tree.
//!
//! The template statements are first written out as HTML in which each
//! `{{ }}`, `{% for %}` and `{% if %}` becomes a `<template>` placeholder,
//! so html5ever parses the markup exactly as a browser would (implied
//! `<html>`/`<body>`, attribute decoding). The placeholders then turn back
//! into template nodes.

use std::cell::RefCell;

use html5ever::parse_document;
use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{Handle, NodeData, RcDom};

use crate::expr::{Cond, Expr, Pattern};
use crate::lower::{self, Stmt};
use crate::source::{Ctx, Error};

pub type NodeId = usize;

/// A parsed template. `nodes[0]` is the document.
#[derive(Clone, Debug)]
pub struct Template {
    pub nodes: Vec<Node>,
    /// The text of its `<style>` elements, in document order.
    pub css: String,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub kind: Kind,
}

#[derive(Clone, Debug)]
pub enum Kind {
    Document,
    /// An HTML element. A `<template>` element of the page itself has its
    /// content as children.
    Element { tag: String, attrs: Vec<Attr> },
    Text(String),
    /// `{{ expr }}` in text.
    Expr(Expr),
    /// `{% for pattern in iter %}`; the children are the body.
    For(Pattern, Expr),
    /// `{% if %}`; the children are its `Branch`es, in order.
    If,
    /// One branch of an `{% if %}`: its test (none for `{% else %}`).
    Branch(Option<Cond>),
    Break,
    Continue,
}

#[derive(Clone, Debug)]
pub struct Attr {
    pub name: String,
    /// The value as written, template code included.
    pub value: String,
    /// The value as parts when it has template code (also for any
    /// `data-key`). `None` for literal values, and for a `class` value that
    /// is not valid template syntax (then its words are the classes).
    pub parts: Option<Vec<AttrPart>>,
}

#[derive(Clone, Debug)]
pub enum AttrPart {
    Lit(String),
    Expr(Expr),
    If(Vec<AttrBranch>),
}

#[derive(Clone, Debug)]
pub struct AttrBranch {
    pub cond: Option<Cond>,
    pub parts: Vec<AttrPart>,
}

impl Template {
    pub const ROOT: NodeId = 0;

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id]
    }

    /// The children of `id`.
    pub fn children(&self, id: NodeId) -> &[NodeId] {
        &self.nodes[id].children
    }
}

impl Attr {
    /// Whether the value has template code.
    pub fn is_dynamic(&self) -> bool {
        is_dynamic(&self.value)
    }
}

pub fn is_dynamic(value: &str) -> bool {
    value.contains("{{") || value.contains("{%")
}

/// A template block written as a `<template>` placeholder.
enum Placeholder {
    Expr(Expr),
    For(Pattern, Expr),
    If(Vec<Option<Cond>>),
}

/// Where the HTML scanner is: in text, inside a tag (where quotes delimit
/// attribute values), or in an HTML comment (where neither tags nor quotes
/// count: an apostrophe in a comment opens nothing).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Markup {
    Text,
    Tag,
    Comment,
}

/// Escape a re-serialized template tag so it survives inside an HTML attribute
/// value quoted with `quote`. html5ever decodes the entities again, so the
/// attribute-level template parser sees the original text.
fn escape_for_attr(s: &str, quote: char) -> String {
    let s = s.replace('&', "&amp;");
    match quote {
        '"' => s.replace('"', "&quot;"),
        _ => s.replace('\'', "&#39;"),
    }
}

struct Preprocess<'c> {
    ctx: &'c Ctx<'c>,
    placeholders: Vec<Placeholder>,
    markup: Markup,
    in_quote: Option<char>,
}

impl Preprocess<'_> {
    /// A template statement that sits inside an HTML start tag. Inside a
    /// quoted attribute value it is re-serialized (and escaped) for the
    /// attribute-level parser; anywhere else in the tag html5ever would shred
    /// it into bogus attribute names, so that is an error.
    fn tag_level(&self, html: &mut String, stmt: &Stmt, what: &str) -> Result<(), Error> {
        match self.in_quote {
            Some(q) => {
                html.push_str(&escape_for_attr(&stmt.to_string(), q));
                Ok(())
            }
            None => Err(self.ctx.error_at(stmt.offset(), format_args!(
                "{what} can only appear inside a quoted attribute value. Write `class=\"{{% if c %}}a{{% else %}}b{{% endif %}}\"` \
                 instead of `{{% if c %}}class=\"a\"{{% else %}}class=\"b\"{{% endif %}}`. \
                 (The only tag-level conditional supported is `{{% if c %}}checked{{% endif %}}`.)"
            ))),
        }
    }

    fn placeholder(&mut self, placeholder: Placeholder) -> usize {
        self.placeholders.push(placeholder);
        self.placeholders.len() - 1
    }

    fn stmts(&mut self, stmts: &[Stmt]) -> Result<String, Error> {
        let mut html = String::new();
        for stmt in stmts {
            match stmt {
                Stmt::Lit(val) => {
                    for (i, c) in val.char_indices() {
                        match self.markup {
                            Markup::Comment => {
                                if c == '>' && val[..i].ends_with("--") {
                                    self.markup = Markup::Text;
                                }
                            }
                            Markup::Tag => {
                                if let Some(quote_char) = self.in_quote {
                                    if c == quote_char {
                                        self.in_quote = None;
                                    }
                                } else if c == '"' || c == '\'' {
                                    self.in_quote = Some(c);
                                } else if c == '>' {
                                    self.markup = Markup::Text;
                                } else if c == '-' && val[..i].ends_with("<!-") {
                                    self.markup = Markup::Comment;
                                }
                            }
                            Markup::Text => {
                                if c == '<' {
                                    self.markup = Markup::Tag;
                                }
                            }
                        }
                    }
                    html.push_str(val);
                }
                Stmt::Expr(expr, _) => {
                    if self.markup == Markup::Tag {
                        self.tag_level(&mut html, stmt, "template expressions")?;
                    } else {
                        let id = self.placeholder(Placeholder::Expr(expr.clone()));
                        html.push_str(&format!("<template expr-id=\"{}\"></template>", id));
                    }
                }
                Stmt::For(pattern, iter, body, _) => {
                    if self.markup == Markup::Tag {
                        self.tag_level(&mut html, stmt, "`{% for %}`")?;
                    } else {
                        let id = self.placeholder(Placeholder::For(pattern.clone(), iter.clone()));
                        html.push_str(&format!("<template loop-id=\"{}\">", id));
                        html.push_str(&self.stmts(body)?);
                        html.push_str("</template>");
                    }
                }
                Stmt::If(branches, _) => {
                    if self.markup == Markup::Tag {
                        // `{% if cond %}checked{% endif %}` is the one tag-level
                        // conditional we support; it becomes `checked="{{ cond }}"`.
                        let checked_cond = match branches.as_slice() {
                            [lower::Branch { cond: Some(Cond { pattern: None, expr }), body }] => match body.as_slice() {
                                [Stmt::Lit(lit)] if lit.trim() == "checked" => Some(expr),
                                _ => None,
                            },
                            _ => None,
                        };
                        match (self.in_quote.is_none(), checked_cond) {
                            (true, Some(expr)) => {
                                let cond = escape_for_attr(&expr.to_string(), '"');
                                html.push_str(&format!(" checked=\"{{{{ {} }}}}\"", cond));
                            }
                            _ => self.tag_level(&mut html, stmt, "`{% if %}`")?,
                        }
                    } else {
                        let id = self.placeholder(Placeholder::If(branches.iter().map(|b| b.cond.clone()).collect()));
                        html.push_str(&format!("<template if-id=\"{}\">", id));
                        for (branch_idx, branch) in branches.iter().enumerate() {
                            html.push_str(&format!("<template branch-id=\"{}\" branch=\"{}\">", id, branch_idx));
                            html.push_str(&self.stmts(&branch.body)?);
                            html.push_str("</template>");
                        }
                        html.push_str("</template>");
                    }
                }
                Stmt::Break(_) => {
                    if self.markup == Markup::Tag {
                        self.tag_level(&mut html, stmt, "`{% break %}`")?;
                    } else {
                        html.push_str("<template break></template>");
                    }
                }
                Stmt::Continue(_) => {
                    if self.markup == Markup::Tag {
                        self.tag_level(&mut html, stmt, "`{% continue %}`")?;
                    } else {
                        html.push_str("<template continue></template>");
                    }
                }
            }
        }
        Ok(html)
    }
}

/// The template's statements as a tree of HTML and template nodes.
pub(crate) fn build(ctx: &Ctx<'_>, stmts: &[Stmt]) -> Result<Template, Error> {
    let mut pre = Preprocess { ctx, placeholders: Vec::new(), markup: Markup::Text, in_quote: None };
    let html = pre.stmts(stmts)?;
    let dom = parse_document(RcDom::default(), Default::default())
        .from_utf8()
        .read_from(&mut html.as_bytes())
        .map_err(|e| Error(format!("failed to parse the template's HTML: {e}")))?;
    let mut builder = Builder { placeholders: pre.placeholders, nodes: Vec::new() };
    builder.add(&dom.document, None)?;
    let mut template = Template { nodes: builder.nodes, css: String::new() };
    template.css = collect_css(&template, Template::ROOT, String::new());
    Ok(template)
}

fn collect_css(template: &Template, id: NodeId, mut css: String) -> String {
    let node = template.node(id);
    if matches!(&node.kind, Kind::Element { tag, .. } if tag == "style") {
        for &child in &node.children {
            if let Kind::Text(text) = &template.node(child).kind {
                css.push_str(text);
                css.push('\n');
            }
        }
    }
    node.children.iter().fold(css, |css, &child| collect_css(template, child, css))
}

/// The children of `handle`; for a `<template>`, its content.
fn dom_children(handle: &Handle) -> Vec<Handle> {
    if let NodeData::Element { name, template_contents, .. } = &handle.data {
        if name.local.as_ref() == "template" {
            if let Some(contents) = template_contents.borrow().as_ref() {
                return contents.children.borrow().clone();
            }
        }
    }
    handle.children.borrow().clone()
}

fn dom_attr(attrs: &RefCell<Vec<html5ever::Attribute>>, name: &str) -> Option<String> {
    attrs.borrow().iter().find(|a| a.name.local.as_ref() == name).map(|a| a.value.to_string())
}

struct Builder {
    placeholders: Vec<Placeholder>,
    nodes: Vec<Node>,
}

impl Builder {
    fn push(&mut self, parent: Option<NodeId>, kind: Kind) -> NodeId {
        let id = self.nodes.len();
        self.nodes.push(Node { parent, children: Vec::new(), kind });
        if let Some(parent) = parent {
            self.nodes[parent].children.push(id);
        }
        id
    }

    fn add_children(&mut self, handles: &[Handle], parent: NodeId) -> Result<(), Error> {
        handles.iter().try_for_each(|child| self.add(child, Some(parent)))
    }

    fn add(&mut self, handle: &Handle, parent: Option<NodeId>) -> Result<(), Error> {
        match &handle.data {
            NodeData::Document => {
                let id = self.push(parent, Kind::Document);
                self.add_children(&dom_children(handle), id)
            }
            NodeData::Text { contents } => {
                self.push(parent, Kind::Text(contents.borrow().to_string()));
                Ok(())
            }
            NodeData::Element { name, attrs, .. } => {
                let tag = name.local.as_ref();
                if tag == "template" {
                    let index = |name| dom_attr(attrs, name).and_then(|v| v.parse::<usize>().ok());
                    if let Some(id) = index("expr-id") {
                        let Placeholder::Expr(expr) = &self.placeholders[id] else { unreachable!() };
                        self.push(parent, Kind::Expr(expr.clone()));
                        return Ok(());
                    }
                    if let Some(id) = index("loop-id") {
                        let Placeholder::For(pattern, iter) = &self.placeholders[id] else { unreachable!() };
                        let node = self.push(parent, Kind::For(pattern.clone(), iter.clone()));
                        return self.add_children(&dom_children(handle), node);
                    }
                    if let Some(id) = index("if-id") {
                        let Placeholder::If(conds) = &self.placeholders[id] else { unreachable!() };
                        let conds = conds.clone();
                        let node = self.push(parent, Kind::If);
                        let branches = dom_children(handle);
                        for (branch_idx, cond) in conds.into_iter().enumerate() {
                            let found = branches.iter().find(|child| match &child.data {
                                NodeData::Element { name, attrs, .. } if name.local.as_ref() == "template" => {
                                    let number = |name| dom_attr(attrs, name).and_then(|v| v.parse::<usize>().ok());
                                    number("branch-id") == Some(id) && number("branch") == Some(branch_idx)
                                }
                                _ => false,
                            });
                            if let Some(branch) = found {
                                let branch_node = self.push(Some(node), Kind::Branch(cond));
                                self.add_children(&dom_children(branch), branch_node)?;
                            }
                        }
                        return Ok(());
                    }
                    if dom_attr(attrs, "break").is_some() {
                        self.push(parent, Kind::Break);
                        return Ok(());
                    }
                    if dom_attr(attrs, "continue").is_some() {
                        self.push(parent, Kind::Continue);
                        return Ok(());
                    }
                }
                let attrs = attrs.borrow().iter()
                    .map(|a| attr(tag, a.name.local.as_ref(), a.value.as_ref()))
                    .collect::<Result<_, _>>()?;
                let id = self.push(parent, Kind::Element { tag: tag.to_string(), attrs });
                self.add_children(&dom_children(handle), id)
            }
            // Comments, doctypes and processing instructions don't render.
            _ => Ok(()),
        }
    }
}

/// Why an attribute value could not be read as template parts.
enum AttrError {
    /// Not valid template syntax.
    Syntax(String),
    /// Valid, but not supported inside an attribute value.
    Unsupported(String),
}

fn attr(tag: &str, name: &str, value: &str) -> Result<Attr, Error> {
    let parse = || -> Result<Vec<AttrPart>, AttrError> {
        let syntax = askama_parser::Syntax::default();
        let ast = askama_parser::Ast::from_str(value, None, &syntax)
            .map_err(|e| AttrError::Syntax(e.message.as_deref().unwrap_or("syntax error").to_string()))?;
        let ctx = Ctx { src: value, map: None };
        let stmts = lower::nodes(&ctx, ast.nodes()).map_err(|e| AttrError::Unsupported(e.0))?;
        attr_parts(stmts).map_err(AttrError::Unsupported)
    };
    let parts = match (name, parse()) {
        // A `class` that isn't template syntax is just its words.
        ("class", Err(AttrError::Syntax(_))) => None,
        (_, Err(AttrError::Unsupported(e))) => return Err(Error(e)),
        (_, Err(AttrError::Syntax(e))) if is_dynamic(value) || name == "data-key" => {
            return Err(Error(format!("failed to parse template syntax in attribute `{name}=\"{value}\"` of <{tag}>: {e}")));
        }
        ("class", Ok(parts)) => {
            // CSS is matched against classes when the template is compiled,
            // so they must be literal.
            if has_expr(&parts) {
                return Err(Error(format!(
                    "class names produced by `{{{{ ... }}}}` (in `class=\"{value}\"`) can't be styled: CSS is matched \
                     at compile time. Use literal class names behind conditions instead: \
                     `class=\"{{% if selected %}}active{{% endif %}}\"`"
                )));
            }
            Some(parts)
        }
        (_, Ok(parts)) if is_dynamic(value) || name == "data-key" => Some(parts),
        _ => None,
    };
    Ok(Attr { name: name.to_string(), value: value.to_string(), parts })
}

fn has_expr(parts: &[AttrPart]) -> bool {
    parts.iter().any(|part| match part {
        AttrPart::Lit(_) => false,
        AttrPart::Expr(_) => true,
        AttrPart::If(branches) => branches.iter().any(|b| has_expr(&b.parts)),
    })
}

fn attr_parts(stmts: Vec<Stmt>) -> Result<Vec<AttrPart>, String> {
    stmts.into_iter().map(|stmt| match stmt {
        Stmt::Lit(text) => Ok(AttrPart::Lit(text)),
        Stmt::Expr(expr, _) => Ok(AttrPart::Expr(expr)),
        Stmt::If(branches, _) => Ok(AttrPart::If(branches.into_iter()
            .map(|b| Ok(AttrBranch { cond: b.cond, parts: attr_parts(b.body)? }))
            .collect::<Result<_, String>>()?)),
        other => Err(format!(
            "only `{{{{ expr }}}}` and `{{% if %}}` are supported inside attribute values, found `{other}`; \
             build the value in Rust instead (e.g. a method on your model)"
        )),
    }).collect()
}
