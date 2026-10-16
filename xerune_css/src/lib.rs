//! The CSS text handling shared by `xerune` (runtime stylesheets) and
//! `xerune_derive` (compiled templates), so both read a stylesheet the
//! same way. A proc-macro crate can't use the crate it generates code for;
//! both can use this one.

#![no_std]
#![warn(missing_docs)]

extern crate alloc;

pub mod media;
pub mod pseudo;
pub mod text;

pub use pseudo::{rewrite_part_rules, ControlPart};

use alloc::string::String;

/// `simplecss` cuts a value off at `/` and loses the rest of the rule, so
/// the `/` of `/<number>` (the line-height in `font: 14px/1.4`) is written
/// as this character (U+2044 FRACTION SLASH, which never occurs in CSS),
/// which the `font` parser accepts.
pub const SLASH: char = '\u{2044}';

/// `css` with each `/` followed by a number replaced by [`SLASH`].
pub fn protect_slashes(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut chars = css.chars().peekable();
    while let Some(c) = chars.next() {
        let before_number = chars.peek().is_some_and(|n| n.is_ascii_digit() || *n == '.');
        out.push(if c == '/' && before_number { SLASH } else { c });
    }
    out
}
