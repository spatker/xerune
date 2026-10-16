//! Scanning CSS text: comments, strings and nesting.

use alloc::string::String;
use alloc::vec::Vec;

/// Byte offset of the first `wanted` in `css` outside comments and strings.
pub fn find_outside_comments(css: &str, wanted: &[u8]) -> Option<usize> {
    let bytes = css.as_bytes();
    let mut i = 0;
    let mut quote = None;
    while i < bytes.len() {
        let b = bytes[i];
        match quote {
            Some(q) if b == q => quote = None,
            Some(_) if b == b'\\' => i += 1,
            Some(_) => {}
            None if b == b'/' && bytes.get(i + 1) == Some(&b'*') => {
                i = css[i + 2..].find("*/").map_or(bytes.len(), |end| i + 2 + end + 1);
            }
            None if b == b'"' || b == b'\'' => quote = Some(b),
            None if wanted.contains(&b) => return Some(i),
            None => {}
        }
        i += 1;
    }
    None
}

/// `css` split at each top-level `separator` (outside parentheses, comments
/// and strings).
pub fn split_top_level(css: &str, separator: u8) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    let mut i = 0;
    while let Some(offset) = find_outside_comments(&css[i..], &[separator, b'(', b')']) {
        let at = i + offset;
        match css.as_bytes()[at] {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => {
                pieces.push(&css[start..at]);
                start = at + 1;
            }
            _ => {}
        }
        i = at + 1;
    }
    pieces.push(&css[start..]);
    pieces
}

/// `css` without its `/* */` comments.
pub fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        rest = rest[start + 2..].find("*/").map_or("", |end| &rest[start + 2 + end + 2..]);
    }
    out.push_str(rest);
    out
}
