//! Lexical comment inspection for the M0 source gate (design §2.4).
//!
//! A slash pair is a Rust comment only when it appears outside a quoted
//! string. The scanner masks string contents before other lint rules inspect
//! code, so a policy example in a diagnostic cannot trip that policy itself.
//! This is a deliberately small lexer for the code written in M0; Rust and
//! Clippy still own syntax validation.
//!
//! The project requires at least forty comment lines per hundred code lines,
//! but a raw search for slash pairs would count `https://` inside strings.
//! Masking quoted contents also lets the forbidden-call rule contain example
//! spellings in its own string constants without exempting its source file.
//!
//! Comment quality is a workspace-wide property. Each normalized explanation
//! is counted across files, so copying the same line into several modules
//! fails even if each file independently clears the density threshold.
//! Empty separator comments are ignored because they carry no explanation.

use std::{collections::BTreeMap, path::Path};

#[cfg(test)]
/// Behavioural regression cases for the comment scanner and quality guard.
mod tests;

/// Tracks normalized comment text across every owned source file.
/// A sorted map gives deterministic first-failure diagnostics during traversal.
#[derive(Default)]
pub(super) struct QualityGuard {
    /// The count for each lowercased, whitespace-normalized comment.
    counts: BTreeMap<String, usize>,
}

/// Applies workspace-wide quality checks as source files are visited.
impl QualityGuard {
    /// Rejects generic text and the fourth appearance of any explanation.
    ///
    /// Case and runs of whitespace do not create distinct explanations; the
    /// threshold applies to what a reader would recognize as the same text.
    pub(super) fn observe(&mut self, path: &Path, text: &str) -> Result<(), String> {
        let normalized = text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        if normalized.is_empty() {
            // A blank doc separator has no text whose repetition can mislead.
            return Ok(());
        }
        // Phrases that explain no specific choice remain invalid even once.
        for phrase in [
            "keep this operation explicit",
            "set this field explicitly",
            "convert failure into a reported result",
            "keep this intermediate value local",
            "apply the repository rule",
            "visit each input once",
            "guard this branch",
            "fail explicitly",
            "handle every outcome",
            "return the completed value",
            "return a diagnostic",
        ] {
            if normalized.contains(phrase) {
                return Err(format!("{}: generic comment: {text}", path.display()));
            }
        }
        let count = self.counts.entry(normalized).or_default();
        // Count after the phrase check so a generic line fails on its first use.
        *count += 1;
        if *count > 3 {
            return Err(format!(
                "{}: comment repeated more than three times: {text}",
                path.display()
            ));
        }
        Ok(())
    }
}

/// Separates executable code from a real line comment while masking literals.
///
/// The returned code keeps punctuation and identifiers, but string contents
/// become spaces. That makes forbidden-call checks inspect syntax rather than
/// examples inside diagnostics. The comment begins only at an unquoted `//`.
/// Escaped quote marks stay within their string, including diagnostic literals.
pub(super) fn scan_line(line: &str) -> (String, Option<&str>) {
    let bytes = line.as_bytes();
    let mut code = String::with_capacity(line.len());
    let mut quoted = false;
    let mut escaped = false;
    let mut index = 0;
    while index < bytes.len() {
        // Byte positions are valid slice boundaries when the byte is ASCII '/'.
        let byte = bytes[index];
        if quoted {
            // Every byte in a literal becomes a space in the active-code view.
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
            code.push(' ');
        } else if byte == b'"' {
            quoted = true;
            code.push(' ');
        } else if byte == b'/' && bytes.get(index + 1) == Some(&b'/') {
            // Only a slash pair outside a literal contributes to comment density.
            return (
                code,
                Some(line[index + 2..].trim_start_matches(['/', '!', ' '])),
            );
        } else {
            code.push(char::from(byte));
        }
        index += 1;
    }
    (code, None)
}
