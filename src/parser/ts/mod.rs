//! Tree-sitter based frontend for TypeScript/JavaScript.
//!
//! CONTRACT: see [`crate::parser`]. Traversal lives in [`extract`].

mod extract;
pub mod unused_imports;

use super::{ImportRecord, Language, SourceLanguage};

/// Frontend backed by the tree-sitter TypeScript/TSX/JS grammar.
pub struct TreeSitterTs {
    lang: SourceLanguage,
}

impl TreeSitterTs {
    /// Create a frontend for the given language variant.
    #[must_use]
    pub fn new(lang: SourceLanguage) -> Self {
        Self { lang }
    }
}

impl Language for TreeSitterTs {
    fn language(&self) -> SourceLanguage {
        self.lang
    }

    fn extract_imports(&self, source: &str) -> Vec<ImportRecord> {
        extract::extract(self.lang, source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(lang: SourceLanguage, source: &str) -> Vec<ImportRecord> {
        TreeSitterTs::new(lang).extract_imports(source)
    }

    /// Assert `records[i]` is specifier `spec` over `source` with quotes
    /// `q`, at the byte span implied by the occurrence of `q.spec.q`.
    #[track_caller]
    fn assert_import(source: &str, records: &[ImportRecord], index: usize, spec: &str) {
        let rec = &records[index];
        assert_eq!(rec.specifier, spec);
        assert_eq!(&source[rec.span.clone()], spec);
        let before = source[..rec.span.start].bytes().last().unwrap_or(b'\n');
        let after = source.as_bytes()[rec.span.end];
        assert!(
            (before == b'"' || before == b'\'') && before == after,
            "span must sit exactly between matching quotes, got {before:?} / {after:?}"
        );
    }

    #[test]
    fn import_statements_with_default_and_named_bindings() {
        let src = "import X from './a'\nimport { A, B } from \"../utils/b\";\n";
        let recs = parse(SourceLanguage::TypeScript, src);
        assert_eq!(recs.len(), 2);
        assert_import(src, &recs, 0, "./a");
        assert_import(src, &recs, 1, "../utils/b");
        assert!(recs.iter().all(|r| !r.is_dynamic));
    }

    #[test]
    fn bare_side_effect_import() {
        let src = "import './styles.css';\n";
        let recs = parse(SourceLanguage::TypeScript, src);
        assert_eq!(recs.len(), 1);
        assert_import(src, &recs, 0, "./styles.css");
        assert!(!recs[0].is_dynamic);
    }

    #[test]
    fn type_only_import_is_static() {
        let src = "import type { A } from './types';\n";
        let recs = parse(SourceLanguage::TypeScript, src);
        assert_eq!(recs.len(), 1);
        assert_import(src, &recs, 0, "./types");
        assert!(!recs[0].is_dynamic);
    }

    #[test]
    fn export_from_reexport() {
        let src = "export { A } from './a';\nexport * from './b';\n";
        let recs = parse(SourceLanguage::TypeScript, src);
        assert_eq!(recs.len(), 2);
        assert_import(src, &recs, 0, "./a");
        assert_import(src, &recs, 1, "./b");
    }

    #[test]
    fn require_call_is_dynamic() {
        let src = "const x = require('./a');\n";
        let recs = parse(SourceLanguage::TypeScript, src);
        assert_eq!(recs.len(), 1);
        assert_import(src, &recs, 0, "./a");
        assert!(recs[0].is_dynamic);
    }

    #[test]
    fn dynamic_import_await_and_chained() {
        let src = "const a = await import('./a');\nimport('./b').then((m) => m);\n";
        let recs = parse(SourceLanguage::TypeScript, src);
        assert_eq!(recs.len(), 2);
        assert_import(src, &recs, 0, "./a");
        assert_import(src, &recs, 1, "./b");
        assert!(recs.iter().all(|r| r.is_dynamic));
    }

    #[test]
    fn declarations_and_comments_are_not_captured() {
        let src = "export const x = 1;\n// import y from './nope'\n/* require('./nah') */\n";
        assert!(parse(SourceLanguage::TypeScript, src).is_empty());
    }

    #[test]
    fn nested_imports_inside_functions_and_blocks() {
        let src = "function f() { if (true) { const m = require('./deep'); } }\n";
        let recs = parse(SourceLanguage::JavaScript, src);
        assert_eq!(recs.len(), 1);
        assert_import(src, &recs, 0, "./deep");
    }

    #[test]
    fn jsx_file_uses_tsx_grammar() {
        // The `require` sits inside a JSX attribute body: only the TSX
        // grammar parses that expression tree (TS reads `<Button` as a
        // type assertion and loses the nested call).
        let src = "export const C = () => <Button onClick={() => require('./a')} />;\n";
        let recs = parse(SourceLanguage::Tsx, src);
        assert_eq!(recs.len(), 1);
        assert_import(src, &recs, 0, "./a");
        assert!(recs[0].is_dynamic);
        assert!(
            parse(SourceLanguage::TypeScript, src).is_empty(),
            "JSX must need the TSX grammar to parse fully"
        );
    }

    #[test]
    fn records_are_in_byte_order_with_distinct_spans() {
        let src = "import a from './a';\nexport * from './b';\nconst c = require('./c');\n";
        let recs = parse(SourceLanguage::TypeScript, src);
        let spans: Vec<_> = recs.iter().map(|r| r.span.start).collect();
        assert_eq!(spans, {
            let mut s = spans.clone();
            s.sort();
            s
        });
        assert_eq!(recs.len(), 3);
    }

    #[test]
    fn plain_javascript_parses_with_the_ts_grammar() {
        let src = "const x = require('./a');\nimport y from './b';\n";
        let recs = parse(SourceLanguage::JavaScript, src);
        assert_eq!(recs.len(), 2);
        assert_import(src, &recs, 1, "./b");
    }
}
