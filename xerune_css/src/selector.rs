//! Selectors: parsing, specificity and matching.
//!
//! Supported: type, universal, class, id and attribute selectors (all six
//! operators and the `i`/`s` flags), the four combinators, the structural
//! pseudo-classes (`:nth-child(An+B of S)`, `:last-of-type`, `:empty`,
//! `:root`, ...), `:not()`, `:is()`, `:where()`, element states
//! (`:checked`, `:hover`, ... answered by [`Element::state`]) and one
//! pseudo-element at the end. Anything else makes the selector invalid, which
//! drops its whole rule, as in browsers.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use crate::pseudo::ControlPart;

/// What a selector needs to know about the element it is matched against.
///
/// Elements are compared by tag case-insensitively, as in HTML.
pub trait Element: Sized {
    /// The parent element, `None` for the root.
    fn parent(&self) -> Option<Self>;
    /// The element before this one among its parent's children.
    fn prev_sibling(&self) -> Option<Self>;
    /// The element after this one among its parent's children.
    fn next_sibling(&self) -> Option<Self>;
    /// The tag name.
    fn tag(&self) -> &str;
    /// The value of attribute `name` (lowercase).
    fn attribute(&self, name: &str) -> Option<Cow<'_, str>>;
    /// Whether `class` is one of the element's classes.
    fn has_class(&self, class: &str) -> bool {
        self.attribute("class").is_some_and(|classes| classes.split_ascii_whitespace().any(|c| c == class))
    }
    /// Whether the element has no children (`:empty`).
    fn is_empty(&self) -> bool {
        false
    }
    /// Whether the element is in `state` (`:checked`, `:hover`, ...).
    fn state(&self, state: State) -> bool {
        let _ = state;
        false
    }
    /// Whether the element stands for elements not known yet (a template's
    /// `{% for %}` that only runs later). It matches nothing, and selectors
    /// that would need to know what it holds (`:nth-child` past it) don't
    /// match either.
    fn is_opaque(&self) -> bool {
        false
    }
}

/// An element state, as a pseudo-class tests it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum State {
    /// `:hover`
    Hover,
    /// `:active`
    Active,
    /// `:focus`
    Focus,
    /// `:focus-visible`
    FocusVisible,
    /// `:focus-within`
    FocusWithin,
    /// `:checked`
    Checked,
    /// `:indeterminate`
    Indeterminate,
    /// `:enabled`
    Enabled,
    /// `:disabled`
    Disabled,
    /// `:link` / `:any-link`
    Link,
    /// `:visited`
    Visited,
    /// `:target`
    Target,
    /// `:placeholder-shown`
    PlaceholderShown,
}

impl State {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "hover" => State::Hover,
            "active" => State::Active,
            "focus" => State::Focus,
            "focus-visible" => State::FocusVisible,
            "focus-within" => State::FocusWithin,
            "checked" => State::Checked,
            "indeterminate" => State::Indeterminate,
            "enabled" => State::Enabled,
            "disabled" => State::Disabled,
            "link" | "any-link" => State::Link,
            "visited" => State::Visited,
            "target" => State::Target,
            "placeholder-shown" => State::PlaceholderShown,
            _ => return None,
        })
    }
}

/// The pseudo-element a selector ends in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PseudoElement {
    /// One xerune draws: `::before`, a slider's thumb, ...
    Part(ControlPart),
    /// A valid one xerune doesn't draw (`::first-line`, `::-webkit-foo`,
    /// a part followed by a state such as `::-webkit-slider-thumb:hover`).
    /// Its rules never apply.
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Combinator {
    /// `a b`
    Descendant,
    /// `a > b`
    Child,
    /// `a + b`
    Next,
    /// `a ~ b`
    Subsequent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttributeOp {
    Exists,
    /// `=`
    Equals,
    /// `~=`
    Includes,
    /// `|=`
    DashMatch,
    /// `^=`
    Prefix,
    /// `$=`
    Suffix,
    /// `*=`
    Substring,
}

#[derive(Debug, Clone)]
enum Condition {
    Id(String),
    Class(String),
    Attribute { name: String, op: AttributeOp, value: String, ignore_case: bool },
    /// `:nth-child(a n + b of S)` and friends, counting from 1.
    Nth { a: i32, b: i32, of_type: bool, from_end: bool, of: Option<Box<[Selector]>> },
    Root,
    Empty,
    State(State),
    Not(Box<[Selector]>),
    Is(Box<[Selector]>),
    /// Valid but never true here (`:lang()`).
    Never,
    /// Always true for elements (`:defined`).
    Always,
}

#[derive(Debug, Clone)]
struct Compound {
    /// How this compound relates to the one before it; unused for the first.
    combinator: Combinator,
    /// The type selector, lowercase; `None` for `*` or none.
    tag: Option<String>,
    conditions: Vec<Condition>,
}

/// A complex selector (`nav > a.on:hover::before`).
#[derive(Debug, Clone)]
pub struct Selector {
    /// Left to right.
    compounds: Vec<Compound>,
    pseudo_element: Option<PseudoElement>,
    specificity: u32,
}

/// Whether something matches, where `None` means it can't be known (it
/// depends on an [`Element::is_opaque`] element).
type Match = Option<bool>;

fn and(a: Match, b: impl FnOnce() -> Match) -> Match {
    match a {
        Some(false) => Some(false),
        Some(true) => b(),
        None => match b() {
            Some(false) => Some(false),
            _ => None,
        },
    }
}

/// Whether any of `matches` is true: unknown if none is but one is unknown.
fn any(matches: impl Iterator<Item = Match>) -> Match {
    let mut result = Some(false);
    for m in matches {
        match m {
            Some(true) => return Some(true),
            None => result = None,
            Some(false) => {}
        }
    }
    result
}

impl Selector {
    /// Parses one complex selector; `None` if it is invalid.
    pub fn parse(text: &str) -> Option<Self> {
        let mut parser = Parser::new(text);
        let selector = parser.complex(true)?;
        parser.skip_whitespace();
        parser.at_end().then_some(selector)
    }

    /// The specificity, packed so that comparing numbers compares
    /// specificities: ids, classes and types in 10 bits each.
    pub fn specificity(&self) -> u32 {
        self.specificity
    }

    /// The pseudo-element the selector selects, if any; [`Self::matches`]
    /// then tells whether the element is its host.
    pub fn pseudo_element(&self) -> Option<PseudoElement> {
        self.pseudo_element
    }

    /// Whether `element` matches (is the host, for a pseudo-element). A
    /// selector of an unsupported pseudo-element matches nothing.
    pub fn matches<E: Element>(&self, element: &E) -> bool {
        self.pseudo_element != Some(PseudoElement::Unsupported) && self.match_at(self.compounds.len() - 1, element) == Some(true)
    }

    /// Whether matching depends on `state` anywhere in the selector.
    pub fn uses_state(&self, state: State) -> bool {
        self.compounds.iter().flat_map(|c| &c.conditions).any(|condition| match condition {
            Condition::State(s) => *s == state,
            Condition::Not(list) | Condition::Is(list) => list.iter().any(|s| s.uses_state(state)),
            Condition::Nth { of: Some(of), .. } => of.iter().any(|s| s.uses_state(state)),
            _ => false,
        })
    }

    fn match_at<E: Element>(&self, index: usize, element: &E) -> Match {
        let compound = &self.compounds[index];
        and(compound.matches(element), || {
            if index == 0 {
                return Some(true);
            }
            let previous = |e: &E| self.match_at(index - 1, e);
            match compound.combinator {
                Combinator::Child => element.parent().map_or(Some(false), |p| previous(&p)),
                Combinator::Next => element.prev_sibling().map_or(Some(false), |s| previous(&s)),
                Combinator::Descendant => any(core::iter::successors(element.parent(), E::parent).map(|e| previous(&e))),
                Combinator::Subsequent => any(core::iter::successors(element.prev_sibling(), E::prev_sibling).map(|e| previous(&e))),
            }
        })
    }

    fn matches_tri<E: Element>(&self, element: &E) -> Match {
        self.match_at(self.compounds.len() - 1, element)
    }
}

impl Compound {
    fn matches<E: Element>(&self, element: &E) -> Match {
        if element.is_opaque() {
            return None;
        }
        if self.tag.as_ref().is_some_and(|tag| !element.tag().eq_ignore_ascii_case(tag)) {
            return Some(false);
        }
        let mut result = Some(true);
        for condition in &self.conditions {
            match condition.matches(element) {
                Some(false) => return Some(false),
                None => result = None,
                Some(true) => {}
            }
        }
        result
    }
}

impl Condition {
    fn matches<E: Element>(&self, element: &E) -> Match {
        Some(match self {
            Condition::Id(id) => element.attribute("id").is_some_and(|v| v == id.as_str()),
            Condition::Class(class) => element.has_class(class),
            Condition::Attribute { name, op, value, ignore_case } => {
                element.attribute(name).is_some_and(|actual| attribute_matches(&actual, *op, value, *ignore_case))
            }
            Condition::Nth { a, b, of_type, from_end, of } => {
                let next = if *from_end { E::next_sibling } else { E::prev_sibling };
                let mut index = 1;
                for sibling in core::iter::successors(next(element), next) {
                    if sibling.is_opaque() {
                        return None;
                    }
                    let counts = !*of_type || sibling.tag().eq_ignore_ascii_case(element.tag());
                    let counts = counts && match of {
                        Some(of) => any(of.iter().map(|s| s.matches_tri(&sibling)))?,
                        None => true,
                    };
                    index += counts as i32;
                }
                if let Some(of) = of {
                    if !any(of.iter().map(|s| s.matches_tri(element)))? {
                        return Some(false);
                    }
                }
                let steps = index - b;
                match *a {
                    0 => steps == 0,
                    a => steps % a == 0 && steps / a >= 0,
                }
            }
            Condition::Root => element.parent().is_none(),
            Condition::Empty => element.is_empty(),
            Condition::State(state) => element.state(*state),
            Condition::Not(list) => !any(list.iter().map(|s| s.matches_tri(element)))?,
            Condition::Is(list) => return any(list.iter().map(|s| s.matches_tri(element))),
            Condition::Never => false,
            Condition::Always => true,
        })
    }
}

fn attribute_matches(actual: &str, op: AttributeOp, value: &str, ignore_case: bool) -> bool {
    let eq = |a: &str, b: &str| if ignore_case { a.eq_ignore_ascii_case(b) } else { a == b };
    let (n, m) = (actual.len(), value.len());
    match op {
        AttributeOp::Exists => true,
        AttributeOp::Equals => eq(actual, value),
        AttributeOp::Includes => !value.is_empty() && actual.split_ascii_whitespace().any(|word| eq(word, value)),
        AttributeOp::DashMatch => {
            eq(actual, value) || (n > m && actual.is_char_boundary(m) && eq(&actual[..m], value) && actual.as_bytes()[m] == b'-')
        }
        AttributeOp::Prefix => m > 0 && n >= m && actual.is_char_boundary(m) && eq(&actual[..m], value),
        AttributeOp::Suffix => m > 0 && n >= m && actual.is_char_boundary(n - m) && eq(&actual[n - m..], value),
        AttributeOp::Substring => {
            m > 0 && if ignore_case {
                actual.to_ascii_lowercase().contains(&value.to_ascii_lowercase())
            } else {
                actual.contains(value)
            }
        }
    }
}

/// Parses a comma-separated selector list; `None` if any selector in it is
/// invalid (the rule is then dropped).
pub fn parse_selector_list(text: &str) -> Option<Vec<Selector>> {
    let mut parser = Parser::new(text);
    let list = parser.list(true)?;
    parser.at_end().then_some(list)
}

const MAX: u32 = 1023;

fn add_specificity(a: u32, b: u32) -> u32 {
    let field = |s: u32, shift: u32| (s >> shift) & MAX;
    [20, 10, 0].iter().map(|&shift| (field(a, shift) + field(b, shift)).min(MAX) << shift).sum()
}

const ID: u32 = 1 << 20;
const CLASS: u32 = 1 << 10;
const TYPE: u32 = 1;

struct Parser<'a> {
    text: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str) -> Self {
        Parser { text, pos: 0 }
    }

    fn rest(&self) -> &'a str {
        &self.text[self.pos..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn at_end(&self) -> bool {
        self.pos == self.text.len()
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.pos += c.len_utf8();
            true
        } else {
            false
        }
    }

    /// Skips whitespace and comments; whether there were any.
    fn skip_whitespace(&mut self) -> bool {
        let start = self.pos;
        loop {
            let rest = self.rest();
            let trimmed = rest.trim_start_matches([' ', '\t', '\n', '\r', '\x0c']);
            self.pos += rest.len() - trimmed.len();
            match trimmed.strip_prefix("/*") {
                Some(comment) => self.pos = comment.find("*/").map_or(self.text.len(), |end| self.text.len() - comment.len() + end + 2),
                None => return self.pos != start,
            }
        }
    }

    /// `a, b, c`. With `top_level`, selectors may end in a pseudo-element.
    fn list(&mut self, top_level: bool) -> Option<Vec<Selector>> {
        let mut list = Vec::new();
        loop {
            list.push(self.complex(top_level)?);
            self.skip_whitespace();
            if !self.eat(',') {
                return Some(list);
            }
        }
    }

    /// A list inside `:is()` / `:where()`, which drops its invalid
    /// selectors instead of failing.
    fn forgiving_list(&mut self) -> Option<Vec<Selector>> {
        let end = self.closing_paren()?;
        let inner = &self.text[self.pos..end];
        let list = crate::text::split_top_level(inner, b',').into_iter().filter_map(Selector::parse)
            .filter(|s| s.pseudo_element.is_none()).collect();
        self.pos = end;
        Some(list)
    }

    /// The offset of the `)` closing the parenthesis we are in.
    fn closing_paren(&self) -> Option<usize> {
        let mut depth = 0usize;
        let mut at = self.pos;
        loop {
            let offset = crate::text::find_outside_comments(&self.text[at..], b"()")?;
            at += offset;
            if self.text.as_bytes()[at] == b'(' {
                depth += 1;
            } else if depth == 0 {
                return Some(at);
            } else {
                depth -= 1;
            }
            at += 1;
        }
    }

    fn complex(&mut self, top_level: bool) -> Option<Selector> {
        self.skip_whitespace();
        let mut compounds = Vec::new();
        let mut specificity = 0;
        let mut combinator = Combinator::Descendant;
        loop {
            let (compound, compound_specificity, pseudo_element) = self.compound(combinator)?;
            compounds.push(compound);
            specificity = add_specificity(specificity, compound_specificity);
            if let Some(pseudo_element) = pseudo_element {
                if !top_level {
                    return None;
                }
                return Some(Selector { compounds, pseudo_element: Some(pseudo_element), specificity: add_specificity(specificity, TYPE) });
            }
            let whitespace = self.skip_whitespace();
            combinator = match self.peek() {
                Some('>') => Combinator::Child,
                Some('+') => Combinator::Next,
                Some('~') => Combinator::Subsequent,
                Some(',' | ')') | None => break,
                Some(_) if whitespace => Combinator::Descendant,
                Some(_) => return None,
            };
            if combinator != Combinator::Descendant {
                self.pos += 1;
                self.skip_whitespace();
            }
        }
        Some(Selector { compounds, pseudo_element: None, specificity })
    }

    /// One compound selector and its specificity, and the pseudo-element
    /// it ends in.
    fn compound(&mut self, combinator: Combinator) -> Option<(Compound, u32, Option<PseudoElement>)> {
        let mut compound = Compound { combinator, tag: None, conditions: Vec::new() };
        let mut specificity = 0;
        let mut empty = true;
        if self.eat('*') {
            empty = false;
        } else if let Some(tag) = self.ident() {
            compound.tag = Some(tag.to_ascii_lowercase());
            specificity += TYPE;
            empty = false;
        }
        if self.peek() == Some('|') {
            // Namespaces aren't supported.
            return None;
        }
        loop {
            match self.peek() {
                Some('#') => {
                    self.pos += 1;
                    // `#1a` is a hash, but not an id.
                    compound.conditions.push(Condition::Id(self.ident()?));
                    specificity = add_specificity(specificity, ID);
                }
                Some('.') => {
                    self.pos += 1;
                    compound.conditions.push(Condition::Class(self.ident()?));
                    specificity = add_specificity(specificity, CLASS);
                }
                Some('[') => {
                    self.pos += 1;
                    compound.conditions.push(self.attribute()?);
                    specificity = add_specificity(specificity, CLASS);
                }
                Some(':') => {
                    self.pos += 1;
                    if self.eat(':') {
                        let pseudo_element = self.pseudo_element()?;
                        return Some((compound, specificity, Some(pseudo_element)));
                    }
                    let name = self.ident()?.to_ascii_lowercase();
                    if matches!(name.as_str(), "before" | "after" | "first-line" | "first-letter") {
                        let pseudo_element = self.after_pseudo_element(&name)?;
                        return Some((compound, specificity, Some(pseudo_element)));
                    }
                    let (condition, condition_specificity) = self.pseudo_class(&name)?;
                    compound.conditions.push(condition);
                    specificity = add_specificity(specificity, condition_specificity);
                }
                _ => break,
            }
            empty = false;
        }
        (!empty).then_some((compound, specificity, None))
    }

    fn pseudo_element(&mut self) -> Option<PseudoElement> {
        let name = self.ident()?.to_ascii_lowercase();
        if self.peek() == Some('(') {
            // `::part()`, `::slotted()`, `::highlight()`.
            let end = self.closing_paren_after_open()?;
            self.pos = end + 1;
            return matches!(name.as_str(), "part" | "slotted" | "highlight").then_some(PseudoElement::Unsupported);
        }
        self.after_pseudo_element(&name)
    }

    fn closing_paren_after_open(&mut self) -> Option<usize> {
        self.pos += 1;
        self.closing_paren()
    }

    /// The pseudo-element `name` (just parsed), and the states that may
    /// follow it (`::-webkit-slider-thumb:hover`), which make it one xerune
    /// doesn't draw.
    fn after_pseudo_element(&mut self, name: &str) -> Option<PseudoElement> {
        const OTHERS: &[&str] = &[
            "first-line", "first-letter", "marker", "selection", "backdrop", "file-selector-button",
            "cue", "grammar-error", "spelling-error", "target-text", "details-content",
        ];
        let mut pseudo_element = match ControlPart::from_pseudo_element(name) {
            Some(part) => PseudoElement::Part(part),
            None if OTHERS.contains(&name) || name.starts_with("-webkit-") || name.starts_with("-moz-") => PseudoElement::Unsupported,
            None => return None,
        };
        while self.eat(':') {
            State::from_name(&self.ident()?.to_ascii_lowercase())?;
            pseudo_element = PseudoElement::Unsupported;
        }
        Some(pseudo_element)
    }

    fn pseudo_class(&mut self, name: &str) -> Option<(Condition, u32)> {
        let nth = |of_type, from_end| Condition::Nth { a: 0, b: 1, of_type, from_end, of: None };
        // `:only-child` is `:is(:first-child:last-child)`, with the
        // specificity of one pseudo-class.
        let only = |of_type| {
            let both = Compound { combinator: Combinator::Descendant, tag: None, conditions: alloc::vec![nth(of_type, false), nth(of_type, true)] };
            Condition::Is(Box::new([Selector { compounds: alloc::vec![both], pseudo_element: None, specificity: 0 }]))
        };
        let simple = match name {
            "first-child" => Some(nth(false, false)),
            "last-child" => Some(nth(false, true)),
            "only-child" => Some(only(false)),
            "first-of-type" => Some(nth(true, false)),
            "last-of-type" => Some(nth(true, true)),
            "only-of-type" => Some(only(true)),
            "root" | "scope" => Some(Condition::Root),
            "empty" => Some(Condition::Empty),
            "defined" => Some(Condition::Always),
            _ => State::from_name(name).map(Condition::State),
        };
        if let Some(condition) = simple {
            return Some((condition, CLASS));
        }
        if !self.eat('(') {
            return None;
        }
        let result = match name {
            "nth-child" | "nth-last-child" | "nth-of-type" | "nth-last-of-type" => {
                let end = self.closing_paren()?;
                let inner = &self.text[self.pos..end];
                let of_type = name.ends_with("of-type");
                let (formula, of) = match find_word(inner, "of") {
                    Some(at) if !of_type => (&inner[..at], Some(&inner[at + 2..])),
                    _ => (inner, None),
                };
                let (a, b) = parse_an_plus_b(formula)?;
                let (of, of_specificity) = match of {
                    Some(of) => {
                        let list = parse_selector_list(of.trim())?;
                        if list.iter().any(|s| s.pseudo_element.is_some()) {
                            return None;
                        }
                        let specificity = list.iter().map(|s| s.specificity).max().unwrap_or(0);
                        (Some(list.into_boxed_slice()), specificity)
                    }
                    None => (None, 0),
                };
                self.pos = end;
                let from_end = name.starts_with("nth-last");
                (Condition::Nth { a, b, of_type, from_end, of }, add_specificity(CLASS, of_specificity))
            }
            "not" | "is" | "where" | "matches" => {
                let list = if name == "not" { self.list(false)? } else { self.forgiving_list()? };
                let specificity = match name {
                    "where" => 0,
                    _ => list.iter().map(|s| s.specificity).max().unwrap_or(0),
                };
                let list = list.into_boxed_slice();
                (if name == "not" { Condition::Not(list) } else { Condition::Is(list) }, specificity)
            }
            "lang" | "dir" => {
                self.pos = self.closing_paren()?;
                (Condition::Never, CLASS)
            }
            _ => return None,
        };
        self.skip_whitespace();
        self.eat(')').then_some(result)
    }

    fn attribute(&mut self) -> Option<Condition> {
        self.skip_whitespace();
        let name = self.ident()?.to_ascii_lowercase();
        if self.peek() == Some('|') && !self.rest().starts_with("|=") {
            return None;
        }
        self.skip_whitespace();
        let op = match self.peek()? {
            ']' => {
                self.pos += 1;
                return Some(Condition::Attribute { name, op: AttributeOp::Exists, value: String::new(), ignore_case: false });
            }
            '=' => AttributeOp::Equals,
            c => {
                let op = match c {
                    '~' => AttributeOp::Includes,
                    '|' => AttributeOp::DashMatch,
                    '^' => AttributeOp::Prefix,
                    '$' => AttributeOp::Suffix,
                    '*' => AttributeOp::Substring,
                    _ => return None,
                };
                self.pos += 1;
                if self.peek() != Some('=') {
                    return None;
                }
                op
            }
        };
        self.pos += 1;
        self.skip_whitespace();
        let value = match self.peek()? {
            '"' | '\'' => self.string()?,
            _ => self.ident()?,
        };
        self.skip_whitespace();
        let ignore_case = match self.peek()? {
            'i' | 'I' => true,
            's' | 'S' => false,
            _ => {
                return self.eat(']').then_some(Condition::Attribute { name, op, value, ignore_case: false });
            }
        };
        self.pos += 1;
        self.skip_whitespace();
        self.eat(']').then_some(Condition::Attribute { name, op, value, ignore_case })
    }

    /// An identifier, unescaped.
    fn ident(&mut self) -> Option<String> {
        if !starts_ident(self.rest()) {
            return None;
        }
        self.name()
    }

    /// A run of name characters (what follows `#`), unescaped.
    fn name(&mut self) -> Option<String> {
        let mut out = String::new();
        while let Some(c) = self.peek() {
            if c == '\\' {
                self.pos += 1;
                out.push(self.escape()?);
            } else if is_name_char(c) {
                out.push(c);
                self.pos += c.len_utf8();
            } else {
                break;
            }
        }
        (!out.is_empty()).then_some(out)
    }

    /// The character an escape (after its `\`) stands for.
    fn escape(&mut self) -> Option<char> {
        let rest = self.rest();
        let hex = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_hexdigit()).len();
        let hex = hex.min(6);
        if hex > 0 {
            let code = u32::from_str_radix(&rest[..hex], 16).ok()?;
            self.pos += hex;
            if let Some(c @ (' ' | '\t' | '\n')) = self.peek() {
                self.pos += c.len_utf8();
            }
            Some(char::from_u32(code).filter(|&c| c != '\0').unwrap_or('\u{fffd}'))
        } else {
            let c = self.peek().filter(|&c| c != '\n')?;
            self.pos += c.len_utf8();
            Some(c)
        }
    }

    /// A quoted string, unescaped.
    fn string(&mut self) -> Option<String> {
        let quote = self.peek()?;
        self.pos += 1;
        let mut out = String::new();
        loop {
            let c = self.peek()?;
            self.pos += c.len_utf8();
            match c {
                c if c == quote => return Some(out),
                '\\' if self.peek() == Some('\n') => self.pos += 1,
                '\\' => out.push(self.escape()?),
                '\n' => return None,
                c => out.push(c),
            }
        }
    }
}

/// Whether `text` starts with an identifier.
pub(crate) fn starts_ident(text: &str) -> bool {
    let mut chars = text.chars();
    match chars.next() {
        Some('-') => matches!(chars.next(), Some(c) if is_name_char(c) && !c.is_ascii_digit() || c == '-' || c == '\\'),
        Some('\\') => true,
        Some(c) => is_name_char(c) && !c.is_ascii_digit(),
        None => false,
    }
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_' || !c.is_ascii()
}

/// The offset of `word` standing alone (between whitespace) in `text`.
fn find_word(text: &str, word: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    text.match_indices(word).map(|(at, _)| at).find(|&at| {
        at > 0 && bytes[at - 1].is_ascii_whitespace() && bytes.get(at + word.len()).is_some_and(|b| b.is_ascii_whitespace())
    })
}

/// `An+B` (`2n+1`, `odd`, `-n + 3`, `5`).
fn parse_an_plus_b(text: &str) -> Option<(i32, i32)> {
    let text = text.trim().to_ascii_lowercase();
    match text.as_str() {
        "odd" => return Some((2, 1)),
        "even" => return Some((2, 0)),
        _ => {}
    }
    let int = |s: &str| -> Option<i32> {
        let digits = s.strip_prefix('+').unwrap_or(s);
        (!digits.is_empty() && digits.trim_start_matches('-').bytes().all(|b| b.is_ascii_digit())).then(|| digits.parse().ok())?
    };
    let Some(n) = text.find('n') else {
        return Some((0, int(&text)?));
    };
    let a = match &text[..n] {
        "" | "+" => 1,
        "-" => -1,
        a => int(a)?,
    };
    let b = text[n + 1..].trim();
    let b = match b.chars().next() {
        None => 0,
        Some(sign @ ('+' | '-')) => {
            let digits = b[1..].trim_start();
            if digits.is_empty() || !digits.bytes().all(|d| d.is_ascii_digit()) {
                return None;
            }
            let value: i32 = digits.parse().ok()?;
            if sign == '-' { -value } else { value }
        }
        Some(_) => return None,
    };
    Some((a, b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    /// A tree of `(tag, attributes, children)` for matching tests.
    struct Node {
        tag: &'static str,
        attrs: Vec<(&'static str, &'static str)>,
        children: Vec<usize>,
        parent: Option<usize>,
    }

    struct Tree(Vec<Node>);

    #[derive(Clone, Copy)]
    struct El<'a>(&'a Tree, usize);

    impl Tree {
        /// Builds a tree from `tag.class#id` items, indented by depth.
        fn new(outline: &str) -> Tree {
            let mut nodes: Vec<Node> = Vec::new();
            let mut stack: Vec<(usize, usize)> = Vec::new();
            for line in outline.lines().filter(|l| !l.trim().is_empty()) {
                let depth = line.len() - line.trim_start().len();
                let spec: &'static str = alloc::boxed::Box::leak(String::from(line.trim()).into_boxed_str());
                let mut parts = spec.split(' ');
                let tag = parts.next().unwrap();
                let attrs = parts.map(|p| p.split_once('=').unwrap_or((p, ""))).collect();
                while stack.last().is_some_and(|&(d, _)| d >= depth) {
                    stack.pop();
                }
                let parent = stack.last().map(|&(_, i)| i);
                let index = nodes.len();
                nodes.push(Node { tag, attrs, children: Vec::new(), parent });
                if let Some(parent) = parent {
                    nodes[parent].children.push(index);
                }
                stack.push((depth, index));
            }
            Tree(nodes)
        }
    }

    impl El<'_> {
        fn sibling(&self, offset: isize) -> Option<Self> {
            let siblings = &self.0.0[self.0.0[self.1].parent?].children;
            let at = siblings.iter().position(|&c| c == self.1)? as isize + offset;
            siblings.get(usize::try_from(at).ok()?).map(|&i| El(self.0, i))
        }
    }

    impl Element for El<'_> {
        fn parent(&self) -> Option<Self> {
            self.0.0[self.1].parent.map(|p| El(self.0, p))
        }
        fn prev_sibling(&self) -> Option<Self> {
            self.sibling(-1)
        }
        fn next_sibling(&self) -> Option<Self> {
            self.sibling(1)
        }
        fn tag(&self) -> &str {
            self.0.0[self.1].tag
        }
        fn attribute(&self, name: &str) -> Option<Cow<'_, str>> {
            self.0.0[self.1].attrs.iter().find(|(k, _)| *k == name).map(|(_, v)| Cow::Borrowed(*v))
        }
        fn is_empty(&self) -> bool {
            self.0.0[self.1].children.is_empty()
        }
        fn state(&self, state: State) -> bool {
            state == State::Checked && self.attribute("checked").is_some()
        }
        fn is_opaque(&self) -> bool {
            self.tag() == "template"
        }
    }

    const TREE: &str = "
html id=html
  body id=body
    nav id=nav class=bar
      a id=a1 href=/x lang=en-US
      a id=a2 class=on
      span id=s1
      a id=a3 data-k=Hello
    ul id=ul
      li id=l1
      li id=l2
      li id=l3 class=x
      li id=l4
      li id=l5 class=x
    p id=p
      input id=box checked
      template id=t
      b id=b
";

    fn matching(selector: &str) -> Vec<String> {
        let tree = Tree::new(TREE);
        let selector = Selector::parse(selector).unwrap_or_else(|| panic!("{selector} should parse"));
        (0..tree.0.len()).map(|i| El(&tree, i)).filter(|e| selector.matches(e))
            .map(|e| String::from(e.attribute("id").unwrap().as_ref())).collect()
    }

    #[test]
    fn simple_selectors_and_combinators() {
        assert_eq!(matching("a"), ["a1", "a2", "a3"]);
        assert_eq!(matching("NAV > A.on"), ["a2"]);
        assert_eq!(matching("body a"), ["a1", "a2", "a3"]);
        assert_eq!(matching("a + span"), ["s1"]);
        assert_eq!(matching("a.on ~ a"), ["a3"]);
        assert_eq!(matching("#nav>*"), ["a1", "a2", "s1", "a3"]);
        assert_eq!(matching("html ul li.x"), ["l3", "l5"]);
        assert_eq!(matching(".bar #a2"), ["a2"]);
        assert_eq!(matching("ul > nav"), Vec::<String>::new());
    }

    #[test]
    fn attribute_selectors() {
        assert_eq!(matching("[href]"), ["a1"]);
        assert_eq!(matching("[lang|=en]"), ["a1"]);
        assert_eq!(matching("[href^='/']"), ["a1"]);
        assert_eq!(matching("[data-k$=lo]"), ["a3"]);
        assert_eq!(matching("[data-k*=\"ell\"]"), ["a3"]);
        assert_eq!(matching("[data-k=hello i]"), ["a3"]);
        assert_eq!(matching("[data-k=hello]"), Vec::<String>::new());
        assert_eq!(matching("[class~=on]"), ["a2"]);
        assert_eq!(matching("[href^='']"), Vec::<String>::new());
    }

    #[test]
    fn structural_pseudo_classes() {
        assert_eq!(matching("li:first-child"), ["l1"]);
        assert_eq!(matching("li:last-child"), ["l5"]);
        assert_eq!(matching("li:nth-child(2n+1)"), ["l1", "l3", "l5"]);
        assert_eq!(matching("li:nth-child(even)"), ["l2", "l4"]);
        assert_eq!(matching("li:nth-child( -n + 2 )"), ["l1", "l2"]);
        assert_eq!(matching("li:nth-last-child(2)"), ["l4"]);
        assert_eq!(matching("li:nth-child(2 of .x)"), ["l5"]);
        assert_eq!(matching("nav a:nth-of-type(3)"), ["a3"]);
        assert_eq!(matching("nav :last-of-type"), ["s1", "a3"]);
        assert_eq!(matching("nav span:only-of-type"), ["s1"]);
        assert_eq!(matching(":root"), ["html"]);
        assert_eq!(matching("li:empty:nth-child(1)"), ["l1"]);
        // Selectors 4: a root has no siblings, so it is an only child.
        assert_eq!(matching(":only-child"), ["html", "body"]);
    }

    #[test]
    fn logical_and_state_pseudo_classes() {
        assert_eq!(matching("nav > :not(a)"), ["s1"]);
        assert_eq!(matching("nav > :not(.on, [href])"), ["s1", "a3"]);
        assert_eq!(matching(":is(span, li.x)"), ["s1", "l3", "l5"]);
        assert_eq!(matching(":where(nav) a:is(.on, :bogus)"), ["a2"]);
        assert_eq!(matching("input:checked"), ["box"]);
        assert_eq!(matching("a:hover"), Vec::<String>::new());
        assert_eq!(matching("a:lang(en)"), Vec::<String>::new());
        let uses_hover = |text| Selector::parse(text).unwrap().uses_state(State::Hover);
        assert!(uses_hover("nav:hover a") && uses_hover("a:not(:is(:hover))") && !uses_hover("a:active"));
    }

    #[test]
    fn opaque_elements_leave_structure_unknown() {
        // `b` may or may not be the last child: the template may expand to
        // something after it... or before it, which hides `b`'s index.
        assert_eq!(matching("p > :last-child"), ["b"]);
        assert_eq!(matching("p > b:nth-child(3)"), Vec::<String>::new());
        assert_eq!(matching("p > b:not(:nth-child(3))"), Vec::<String>::new());
        assert_eq!(matching("template + b"), Vec::<String>::new());
        assert_eq!(matching("input ~ b"), ["b"]);
        assert_eq!(matching("p > :first-child"), ["box"]);
    }

    #[test]
    fn pseudo_elements() {
        let thumb = Selector::parse("input::-webkit-slider-thumb").unwrap();
        assert_eq!(thumb.pseudo_element(), Some(PseudoElement::Part(ControlPart::SliderThumb)));
        assert_eq!(Selector::parse("p:before").unwrap().pseudo_element(), Some(PseudoElement::Part(ControlPart::Before)));
        assert_eq!(Selector::parse("p::first-line").unwrap().pseudo_element(), Some(PseudoElement::Unsupported));
        assert_eq!(Selector::parse("p::-webkit-foo").unwrap().pseudo_element(), Some(PseudoElement::Unsupported));
        assert_eq!(Selector::parse("i::-moz-range-thumb:hover").unwrap().pseudo_element(), Some(PseudoElement::Unsupported));
        assert_eq!(matching("p::first-line"), Vec::<String>::new());
        assert_eq!(matching("#box::before"), ["box"]);
        assert!(Selector::parse("p::bogus").is_none());
        assert!(Selector::parse("p::before span").is_none());
        assert!(Selector::parse(":not(p::before)").is_none());
    }

    #[test]
    fn invalid_selectors() {
        for invalid in ["", "a >", "> a", "a,", "a:bogus", "a::", "[x", "[x=]", "#", ".1a", "a b:nth-child(x)", "ns|a", "a:not()", "a{", "#1a", "#-1"] {
            assert!(parse_selector_list(invalid).is_none(), "{invalid:?} should be invalid");
        }
        assert_eq!(parse_selector_list("a , b>c,d~e").map(|l| l.len()), Some(3));
        assert!(Selector::parse(r"#\31 0").is_some());
        assert_eq!(matching(r".o\n"), ["a2"]);
        assert_eq!(matching(r"#a\31"), ["a1"]);
        assert_eq!(parse_selector_list("a /* x */ , b").map(|l| l.len()), Some(2));
    }

    #[test]
    fn specificity() {
        let s = |text| Selector::parse(text).unwrap().specificity();
        assert_eq!(s("*"), 0);
        assert_eq!(s("li"), TYPE);
        assert_eq!(s("ul li.x"), 2 * TYPE + CLASS);
        assert_eq!(s("#a:hover::before"), ID + CLASS + TYPE);
        assert_eq!(s(":not(#a, .b)"), ID);
        assert_eq!(s(":where(#a) p"), TYPE);
        assert_eq!(s(":nth-child(2 of #a)"), ID + CLASS);
        assert_eq!(s("a:first-child"), CLASS + TYPE);
    }
}
