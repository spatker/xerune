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
            // An unquoted `url( ... )` is one token, whatever it holds.
            None if (b | 0x20) == b'u' && unquoted_url(css, i) => {
                match css[i..].find(')') {
                    Some(end) => i += end,
                    None => return None,
                }
            }
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

/// Whether an unquoted `url(` starts at `at`.
fn unquoted_url(css: &str, at: usize) -> bool {
    let bytes = css.as_bytes();
    let after_name = at == 0 || !(bytes[at - 1].is_ascii_alphanumeric() || matches!(bytes[at - 1], b'-' | b'_' | b'\\') || bytes[at - 1] >= 0x80);
    after_name
        && css.get(at..at + 4).is_some_and(|name| name.eq_ignore_ascii_case("url("))
        && !css[at + 4..].trim_start().starts_with(['"', '\''])
}

/// `css` with each `/* */` comment (outside strings) replaced by a space,
/// which keeps the tokens on either side apart.
pub fn strip_comments(css: &str) -> String {
    let bytes = css.as_bytes();
    let mut out = String::with_capacity(css.len());
    let mut copied = 0;
    let mut i = 0;
    let mut quote = None;
    while i < bytes.len() {
        let b = bytes[i];
        match quote {
            Some(q) if b == q => quote = None,
            Some(_) if b == b'\\' => i += 1,
            Some(_) => {}
            None if b == b'"' || b == b'\'' => quote = Some(b),
            None if b == b'/' && bytes.get(i + 1) == Some(&b'*') => {
                out.push_str(&css[copied..i]);
                out.push(' ');
                i = css[i + 2..].find("*/").map_or(bytes.len(), |end| i + 2 + end + 2);
                copied = i;
                continue;
            }
            None => {}
        }
        i += 1;
    }
    out.push_str(&css[copied.min(css.len())..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_strings_and_comments_hide_what_they_hold() {
        assert_eq!(find_outside_comments("a: url( { x ); b {", b"{"), Some(17));
        assert_eq!(find_outside_comments("a: url('{') {", b"{"), Some(12));
        assert_eq!(find_outside_comments("curl({) {", b"{"), Some(5));
        assert_eq!(find_outside_comments("'{' /* { */ {", b"{"), Some(12));
        assert_eq!(strip_comments("a/**/b '/* x */' /* y"), "a b '/* x */'  ");
        assert_eq!(strip_comments("1/2"), "1/2");
    }
}
