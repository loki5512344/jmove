//! Language frontends: import extraction and module specifier resolution.
//!
//! ## Contract (stable across submodules — implementers must not change it)
//!
//! - A [`Language`] parses one source file into [`ImportRecord`]s: every
//!   *static-ish* module reference (TS `import`/`export from`/`require`/
//!   dynamic `import()`; Java single-type and static-member `import`s —
//!   on-demand `pkg.*` imports are deliberately not extracted).
//! - Java files additionally expose their [`PackageDecl`] via
//!   [`Language::extract_package`]; resolution of Java specifiers goes
//!   through [`java::JavaClassIndex`] instead of [`resolve`] (TS relative
//!   specifiers). External Java imports (jdk, third-party) simply never
//!   appear in the class index.
//! - Rewrites must touch **only the specifier string**, never the rest of
//!   the statement (KISS + no formatter dependency): that is why
//!   [`ImportRecord::span`] is a byte range into the original source.

use std::ops::Range;
use std::path::Path;

pub mod java;
pub mod resolve;
pub mod ts;

/// Source languages `jmove` understands (TS/JS in Phase 1, Java in Phase 1.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceLanguage {
    /// `.ts` (non-TSX) sources.
    TypeScript,
    /// `.tsx` / `.jsx` sources.
    Tsx,
    /// Plain `.js` / `.mjs` / `.cjs` sources.
    JavaScript,
    /// `.java` sources.
    Java,
}

impl SourceLanguage {
    /// Map a file extension (lowercase, no dot) to a language, if supported.
    #[must_use]
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "ts" | "mts" | "cts" => Some(Self::TypeScript),
            "tsx" | "jsx" => Some(Self::Tsx),
            "js" | "mjs" | "cjs" => Some(Self::JavaScript),
            "java" => Some(Self::Java),
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
    /// Raw specifier text as written, e.g. `"../utils/format"` (TS) or
    /// `"com.example.utils.Parser"` (Java).
    pub specifier: String,
    /// Byte range of the *specifier string contents* (inside the quotes,
    /// without the quote characters) in the parsed file. The rewriter
    /// replaces exactly this span and nothing else.
    pub span: Range<usize>,
    /// `true` for dynamic `import("...")` / `require("...")` occurrences.
    pub is_dynamic: bool,
}

/// A Java `package` declaration: dotted name plus the byte span of the name
/// (quotes have no meaning here — the span covers `com.example.utils`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageDecl {
    /// Declared package name, e.g. `"com.example.utils"`.
    pub name: String,
    /// Byte range of the package name in the parsed file.
    pub span: Range<usize>,
}

/// A language frontend that extracts imports from source text.
pub trait Language: Send + Sync {
    /// The language this frontend handles.
    fn language(&self) -> SourceLanguage;

    /// Extract all import records from `source` in byte-offset order.
    /// Parse errors must not be fatal: return what was understood.
    fn extract_imports(&self, source: &str) -> Vec<ImportRecord>;

    /// Extract the `package` declaration, if this language has one and the
    /// file declares it. Default: no package concept (TS/JS).
    fn extract_package(&self, _source: &str) -> Option<PackageDecl> {
        None
    }
}

/// Build the default frontend for `lang`.
#[must_use]
pub fn frontend_for(lang: SourceLanguage) -> Box<dyn Language> {
    match lang {
        SourceLanguage::Java => Box::new(java::TreeSitterJava::new()),
        SourceLanguage::TypeScript | SourceLanguage::Tsx | SourceLanguage::JavaScript => {
            Box::new(ts::TreeSitterTs::new(lang))
        }
    }
}
