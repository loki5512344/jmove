//! Language frontends: import extraction and module specifier resolution.
//!
//! ## Contract (stable across submodules — implementers must not change it)
//!
//! - A [`Language`] parses one source file into [`ImportRecord`]s: every
//!   *static-ish* module reference (TS `import`/`export from`/`require`/
//!   dynamic `import()`).
//! - [`crate::core::parser_support::resolve`] turns a specifier into a
//!   project-relative file path using a resolver aware of the indexed file
//!   set. Non-project (package/bare) specifiers resolve to `None`.
//! - Rewrites must touch **only the specifier string**, never the rest of
//!   the statement (KISS + no formatter dependency): that is why
//!   [`ImportRecord::span`] is a byte range into the original source.

use std::path::Path;

pub mod resolve;
pub mod ts;

/// Source languages `jmove` understands (Phase 1: TypeScript/JavaScript).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceLanguage {
    /// `.ts` (non-TSX) sources.
    TypeScript,
    /// `.tsx` / `.jsx` sources.
    Tsx,
    /// Plain `.js` / `.mjs` / `.cjs` sources.
    JavaScript,
}

impl SourceLanguage {
    /// Map a file extension (lowercase, no dot) to a language, if supported.
    #[must_use]
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "ts" | "mts" | "cts" => Some(Self::TypeScript),
            "tsx" | "jsx" => Some(Self::Tsx),
            "js" | "mjs" | "cjs" => Some(Self::JavaScript),
            _ => None,
        }
    }

    /// Detect the language from a file name. `None` means "not a source file
    /// we index" (skip it).
    #[must_use]
    pub fn for_path(path: &Path) -> Option<Self> {
        Self::from_extension(
            path.extension()
                .and_then(|e| e.to_str())
                .map(str::to_ascii_lowercase)
                .as_deref()?,
        )
    }
}

/// One module reference found in a source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportRecord {
    /// Raw specifier text as written, e.g. `"../utils/format"`.
    pub specifier: String,
    /// Byte range of the *specifier string contents* (inside the quotes,
    /// without the quote characters) in the parsed file. The rewriter
    /// replaces exactly this span and nothing else.
    pub span: std::ops::Range<usize>,
    /// `true` for dynamic `import("...")` / `require("...")` occurrences.
    pub is_dynamic: bool,
}

/// A language frontend that extracts imports from source text.
pub trait Language: Send + Sync {
    /// The language this frontend handles.
    fn language(&self) -> SourceLanguage;

    /// Extract all import records from `source` in byte-offset order.
    /// Parse errors must not be fatal: return what was understood.
    fn extract_imports(&self, source: &str) -> Vec<ImportRecord>;
}

/// Build the default frontend for `lang`.
#[must_use]
pub fn frontend_for(lang: SourceLanguage) -> Box<dyn Language> {
    Box::new(ts::TreeSitterTs::new(lang))
}
