//! Checkstyle XML emission: the de-facto report format for IDEs (IntelliJ,
//! VSCode), Jenkins warnings-ng and GitLab code-quality parsing.
//!
//! `source` carries the jmove rule id namespaced (`jmove.java.unused-import`)
//! so consumers can group by rule. Output is deterministic: files sorted
//! (input is already sorted), errors in source order.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::Violation;

/// Render `violations` as a Checkstyle XML document (always valid, even
/// with zero errors).
#[must_use]
pub fn build(violations: &[Violation]) -> String {
    let mut out =
        String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<checkstyle version=\"10.0\">\n");
    let mut by_file: BTreeMap<&str, Vec<&Violation>> = BTreeMap::new();
    for v in violations {
        by_file.entry(&v.file).or_default().push(v);
    }
    for (file, entries) in by_file {
        let _ = writeln!(out, "  <file name=\"{}\">", escape(file));
        for v in entries {
            let _ = writeln!(
                out,
                "    <error line=\"{}\" severity=\"{}\" message=\"{}\" source=\"{}\"/>",
                v.line,
                v.severity,
                escape(&v.message),
                source(v.rule)
            );
        }
        out.push_str("  </file>\n");
    }
    out.push_str("</checkstyle>\n");
    out
}

// The rule as a dotted pseudo-class name, Checkstyle style.
fn source(rule: &str) -> String {
    format!("jmove.{}", rule.replace('/', "."))
}

/// XML attribute escaping; `&` first so entities survive.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn violation(rule: &'static str, severity: &'static str, file: &str, line: usize) -> Violation {
        Violation {
            rule,
            severity,
            message: "he said \"fix <this> & now\"".to_owned(),
            file: file.to_owned(),
            line,
            fixable: true,
        }
    }

    #[test]
    fn groups_files_and_renders_error_attributes() {
        let xml = build(&[
            violation("java/unused-import", "warning", "a.java", 1),
            violation("java/unused-import", "warning", "a.java", 4),
            violation("broken-import", "error", "b.ts", 2),
        ]);
        assert!(
            xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<checkstyle"),
            "{xml}"
        );
        assert_eq!(xml.matches("<file ").count(), 2);
        assert_eq!(xml.matches("<error ").count(), 3);
        assert!(
            xml.contains("<error line=\"4\" severity=\"warning\""),
            "{xml}"
        );
        assert!(xml.contains("source=\"jmove.java.unused-import\""), "{xml}");
        assert!(xml.contains("source=\"jmove.broken-import\""), "{xml}");
    }

    #[test]
    fn attributes_are_escaped() {
        let xml = build(&[violation("broken-import", "error", "a&b/c.ts", 1)]);
        assert!(
            xml.contains("message=\"he said &quot;fix &lt;this&gt; &amp; now&quot;\""),
            "{xml}"
        );
        assert!(xml.contains("<file name=\"a&amp;b/c.ts\">"), "{xml}");
    }

    #[test]
    fn clean_run_is_a_valid_empty_document() {
        let xml = build(&[]);
        assert_eq!(
            xml,
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<checkstyle version=\"10.0\">\n</checkstyle>\n"
        );
    }
}
