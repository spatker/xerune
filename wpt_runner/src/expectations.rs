//! `expectations.txt`: the tests that are known to fail. Every other test
//! that runs is expected to pass. The header carries a per-suite summary so
//! progress shows up in `git diff`.

use std::{collections::{BTreeMap, BTreeSet}, fmt::Write, fs, path::{Path, PathBuf}};
use crate::{Status, suite_of};

pub struct Expectations {
    failing: BTreeSet<PathBuf>,
}

pub struct Changes {
    regressions: Vec<(PathBuf, String)>,
    fixed: Vec<PathBuf>,
}

impl Expectations {
    pub fn read(file: &Path) -> Result<Self, String> {
        let text = match fs::read_to_string(file) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(format!("{}: {e}", file.display())),
        };
        let mut failing = BTreeSet::new();
        for line in text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
            let path = line.strip_prefix("FAIL ").ok_or_else(|| format!("{}: bad line: {line}", file.display()))?;
            failing.insert(PathBuf::from(path.trim()));
        }
        Ok(Self { failing })
    }

    /// Only tests present in `results` are compared, so filtered runs work.
    pub fn compare(&self, results: &BTreeMap<PathBuf, Status>) -> Changes {
        let mut changes = Changes { regressions: Vec::new(), fixed: Vec::new() };
        for (path, status) in results {
            match status {
                Status::Fail(reason) if !self.failing.contains(path) => changes.regressions.push((path.clone(), reason.clone())),
                Status::Pass if self.failing.contains(path) => changes.fixed.push(path.clone()),
                _ => {}
            }
        }
        changes
    }

    pub fn write(file: &Path, header: &str, suites: &[String], results: &BTreeMap<PathBuf, Status>) -> Result<(), String> {
        let mut out = String::new();
        writeln!(out, "# Xerune WPT expectations: the reftests that fail. Tests that run and are").unwrap();
        writeln!(out, "# not listed pass. Regenerate with").unwrap();
        writeln!(out, "#   WPT_DIR=... cargo run --release -p wpt_runner -- --update").unwrap();
        writeln!(out, "# {header}").unwrap();
        writeln!(out, "#").unwrap();
        for line in summary_table(results, suites).lines() {
            writeln!(out, "# {line}").unwrap();
        }
        for (path, status) in results {
            if matches!(status, Status::Fail(_)) {
                writeln!(out, "FAIL {}", path.display()).unwrap();
            }
        }
        fs::write(file, out).map_err(|e| format!("{}: {e}", file.display()))
    }
}

impl Changes {
    pub fn has_regressions(&self) -> bool {
        !self.regressions.is_empty()
    }

    pub fn print(&self) {
        if !self.fixed.is_empty() {
            println!("\n{} tests now pass (run with --update to record):", self.fixed.len());
            for path in &self.fixed {
                println!("  {}", path.display());
            }
        }
        if !self.regressions.is_empty() {
            println!("\n{} tests fail that are expected to pass:", self.regressions.len());
            for (path, reason) in &self.regressions {
                println!("  {}: {reason}", path.display());
            }
        }
    }
}

/// Pass rate per suite. Skipped tests are not part of the rate.
pub fn summary_table(results: &BTreeMap<PathBuf, Status>, suites: &[String]) -> String {
    let mut counts: BTreeMap<&str, [usize; 3]> = suites.iter().map(|s| (s.as_str(), [0; 3])).collect();
    for (path, status) in results {
        let index = match status { Status::Pass => 0, Status::Fail(_) => 1, Status::Skip(_) => 2 };
        counts.entry(suite_of(path, suites)).or_default()[index] += 1;
    }
    let mut total = [0; 3];
    let mut out = format!("{:<34}{:>6}{:>6}{:>7}{:>7}\n", "suite", "pass", "fail", "pass%", "skip");
    let mut row = |name: &str, [pass, fail, skip]: [usize; 3]| {
        let rate = if pass + fail == 0 { "-".into() } else { format!("{:.0}%", 100.0 * pass as f64 / (pass + fail) as f64) };
        writeln!(out, "{name:<34}{pass:>6}{fail:>6}{rate:>7}{skip:>7}").unwrap();
    };
    for (suite, c) in &counts {
        row(suite, *c);
        for i in 0..3 { total[i] += c[i]; }
    }
    row("total", total);
    out
}
