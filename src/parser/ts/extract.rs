//! Tree-sitter traversal backing [`crate::parser::ts`] and its rules.
//!
//! Finds every module reference: `import`/`export … from` statements (the
//! grammar exposes the specifier via the `source` field), `require("…")`
//! calls and dynamic `import("…")` calls. Nesting depth is arbitrary, so
//! the walk is fully recursive.

use tree_sitter::{Language, Node, Parser};

use super::{ImportRecord, SourceLanguage};

/// Record for a `string` node used as a specifier: the span covers the
/// text between the quote characters (the grammar only emits ASCII
/// `"`/`'` quotes for import sources), never the quotes themselves.
fn record(string: Node, source: &str, is_dynamic: bool) -> Option<ImportRecord> {
    let (start, end) = (string.start_byte(), string.end_byte());
    let text = &source[start..end];
    let quote = *text.as_bytes().first()?;
    if quote != b'"' && quote != b'\'' || text.as_bytes().last() != Some(&quote) {
        return None;
    }
    Some(ImportRecord {
        specifier: text[1..text.len() - 1].to_string(),
        span: start + 1..end - 1,
        is_dynamic,
    })
}

/// Record for a `require("…")` / `import("…")` call whose first argument
/// is a string literal. Both are flagged `is_dynamic` per the contract in
/// [`crate::parser`].
fn call_record(call: Node, source: &str) -> Option<ImportRecord> {
    let func = call.child_by_field_name("function")?;
    let is_require = func.kind() == "identifier"
        && source.get(func.start_byte()..func.end_byte()) == Some("require");
    if func.kind() != "import" && !is_require {
        return None;
    }
    let args = call.child_by_field_name("arguments")?;
    let first = args.named_child(0).filter(|n| n.kind() == "string")?;
    record(first, source, true)
}

/// Pre-order walk pushing every module reference under `node`.
fn visit(node: Node, source: &str, out: &mut Vec<ImportRecord>) {
    match node.kind() {
        // `import "./a.css"` and `export * from "./a"` also carry a
        // `source` field; `export const x = …` has none and is skipped.
        "import_statement" | "export_statement" => out.extend(
            node.child_by_field_name("source")
                .and_then(|string| record(string, source, false)),
        ),
        "call_expression" => out.extend(call_record(node, source)),
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        visit(child, source, out);
    }
}

/// Grammar for a language variant. The TypeScript grammar also parses
/// plain JavaScript; only JSX needs the TSX variant. Java never reaches
/// this frontend ([`crate::parser::frontend_for`] routes it to
/// [`crate::parser::java`]), so everything else maps to plain TypeScript.
pub(super) fn grammar(lang: SourceLanguage) -> Language {
    match lang {
        SourceLanguage::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        _ => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
    }
}

/// Parse `source` and collect its imports in byte-offset order.
/// A parse failure yields whatever the partial tree understood (possibly
/// nothing); it never panics.
pub(super) fn extract(lang: SourceLanguage, source: &str) -> Vec<ImportRecord> {
    // tree-sitter's Parser holds raw pointers; build one per call instead
    // of storing it in the (Sync) frontend struct.
    let mut parser = Parser::new();
    let grammar = grammar(lang);
    if parser.set_language(&grammar).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(source, None) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    visit(tree.root_node(), source, &mut out);
    out.sort_by_key(|rec| rec.span.start);
    out
}
