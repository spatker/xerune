//! Template files, `{% include %}` expansion, and error locations.

use std::path::{Path, PathBuf};

/// A template file: its name in messages, the directory its includes are
/// looked up from, and its text.
pub struct File {
    pub name: String,
    pub dir: PathBuf,
    pub content: String,
}

/// A template error, with its `file:line:col` and the source line when it
/// points into the template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

/// A contiguous chunk of the include-expanded template that came from one file.
struct Segment {
    start: usize,
    file: usize,
    file_offset: usize,
}

/// Maps offsets in the include-expanded template back to the original files.
#[derive(Default)]
pub(crate) struct SourceMap {
    /// (name, contents)
    files: Vec<(String, String)>,
    segments: Vec<Segment>,
}

impl SourceMap {
    fn add_file(&mut self, name: String, content: String) -> usize {
        self.files.push((name, content));
        self.files.len() - 1
    }

    fn describe_file_offset(&self, file: usize, offset: usize) -> String {
        let (path, content) = &self.files[file];
        let offset = offset.min(content.len());
        let before = &content[..offset];
        let line_no = before.matches('\n').count() + 1;
        let line_start = before.rfind('\n').map_or(0, |i| i + 1);
        let line_end = content[offset..].find('\n').map_or(content.len(), |i| offset + i);
        let line = &content[line_start..line_end];
        let col = content[line_start..offset].chars().count() + 1;
        let gutter = " ".repeat(line_no.to_string().len());
        format!(
            "{path}:{line_no}:{col}\n{gutter} |\n{line_no} | {line}\n{gutter} | {caret:>width$}",
            caret = "^",
            width = col,
        )
    }

    /// Describe an offset into the include-expanded template.
    pub(crate) fn describe(&self, offset: usize) -> String {
        match self.segments.iter().rev().find(|s| s.start <= offset) {
            Some(seg) => self.describe_file_offset(seg.file, seg.file_offset + (offset - seg.start)),
            None => format!("<template>@{offset}"),
        }
    }
}

/// Where errors point: the text being parsed and, for the template itself
/// (not an attribute value parsed on its own), its source map.
#[derive(Clone, Copy)]
pub(crate) struct Ctx<'a> {
    pub src: &'a str,
    pub map: Option<&'a SourceMap>,
}

impl Ctx<'_> {
    /// `msg`, pointing at `offset` in the template.
    pub(crate) fn error_at(&self, offset: usize, msg: impl std::fmt::Display) -> Error {
        match self.map {
            Some(map) => Error(format!("{msg}\n  --> {}", map.describe(offset))),
            None => Error(msg.to_string()),
        }
    }

    /// `msg`, pointing at `span` if it lies in the template.
    pub(crate) fn error(&self, span: askama_parser::Span<'_>, msg: impl std::fmt::Display) -> Error {
        match self.offset(span) {
            Some(offset) => self.error_at(offset, msg),
            None => Error(msg.to_string()),
        }
    }

    pub(crate) fn offset(&self, span: askama_parser::Span<'_>) -> Option<usize> {
        self.map?;
        span.offset_from(self.src)
    }
}

/// Loads an included template: its path as written, and the directory of
/// the including file.
pub type Loader<'l> = dyn FnMut(&str, &Path) -> Result<File, String> + 'l;

/// `root` with its includes inlined at text level, and where each chunk of
/// the result came from.
pub(crate) fn expand_includes(root: File, load: &mut Loader<'_>) -> Result<(String, SourceMap), Error> {
    let mut map = SourceMap::default();
    let mut out = String::new();
    let dir = root.dir;
    let file = map.add_file(root.name, root.content);
    expand(file, &dir, load, &mut out, &mut map)?;
    Ok((out, map))
}

fn expand(file: usize, dir: &Path, load: &mut Loader<'_>, out: &mut String, map: &mut SourceMap) -> Result<(), Error> {
    let content = map.files[file].1.clone();
    let mut pos = 0;
    while let Some(rel_idx) = content[pos..].find("{% include") {
        let start_idx = pos + rel_idx;
        map.segments.push(Segment { start: out.len(), file, file_offset: pos });
        out.push_str(&content[pos..start_idx]);
        let rest = &content[start_idx..];
        let Some(end_idx) = rest.find("%}") else {
            return Err(Error(format!("unterminated `{{% include %}}`\n  --> {}", map.describe_file_offset(file, start_idx))));
        };
        let path_str = rest["{% include".len()..end_idx].trim().trim_matches(|c| c == '"' || c == '\'');
        if path_str.is_empty() {
            return Err(Error(format!("expected `{{% include \"path\" %}}`\n  --> {}", map.describe_file_offset(file, start_idx))));
        }
        let included = load(path_str, dir).map_err(|e| Error(format!("{e}\n  --> {}", map.describe_file_offset(file, start_idx))))?;
        let included_dir = included.dir;
        let included = map.add_file(included.name, included.content);
        expand(included, &included_dir, load, out, map)?;
        pos = start_idx + end_idx + 2;
    }
    map.segments.push(Segment { start: out.len(), file, file_offset: pos });
    out.push_str(&content[pos..]);
    Ok(())
}
