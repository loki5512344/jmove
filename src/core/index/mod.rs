//! Project indexing: gitignore-aware filesystem scan plus the import graph.
//!
//! Built fresh on every command (disk cache is Phase 2). `ignore::WalkBuilder`
//! handles `.gitignore`/hidden-file rules; every indexed source file is parsed
//! through [`crate::parser`] and its specifiers resolved — TS/JS relative
//! specifiers via [`crate::parser::resolve`], Java FQNs via the declared
//! package map ([`crate::parser::java`]).

mod files;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

use crate::core::{JmoveResult, normalize_rel_path};
use crate::parser::java::JavaClassIndex;
use crate::parser::resolve::resolve_module;
use crate::parser::{ImportRecord, Language, PackageDecl, SourceLanguage, frontend_for};

pub use files::FileSet;

/// One import occurrence plus the project file it resolves to.
/// `target: None` means "external" — a bare package specifier or a path
/// that does not exist in the index.
#[derive(Debug, Clone)]
pub struct ResolvedImport {
    /// Raw record from the parser (specifier text + byte span).
    pub record: ImportRecord,
    /// Project-relative resolved file, if any.
    pub target: Option<PathBuf>,
}

/// Full in-memory project index: file set and forward import edges.
#[derive(Debug, Default)]
pub struct Index {
    /// Absolute project root the index was built for.
    pub root: PathBuf,
    /// All indexed source files.
    pub files: FileSet,
    /// For each file, the imports it declares (in source order).
    pub imports: HashMap<PathBuf, Vec<ResolvedImport>>,
    /// Declared `package` of each Java file that has one.
    pub packages: HashMap<PathBuf, PackageDecl>,
    /// FQN → file map for every indexed Java class; the fix rules use it
    /// for candidate lookup (empty when the project has no Java sources).
    pub java_classes: JavaClassIndex,
}

impl Index {
    /// Scan `root`, parse every supported source file and build the graph.
    /// Unreadable or unparseable files are skipped, not fatal.
    pub fn build(root: &Path) -> JmoveResult<Self> {
        Self::build_scoped(root, None)
    }

    /// Like [`build`](Self::build), but when `source_root` (project-relative,
    /// normalized) is `Some`, only files under that subtree are indexed. This
    /// is the monorepo escape hatch: `guava` vs `android/guava` declare the
    /// same FQNs, and scoping the index to one self-contained copy makes
    /// collision-free resolution (and therefore `mv`/`fix` rewrites) exact.
    pub fn build_scoped(root: &Path, source_root: Option<&Path>) -> JmoveResult<Self> {
        let root = root.canonicalize()?;
        let mut index = Self {
            root,
            files: FileSet::default(),
            imports: HashMap::new(),
            packages: HashMap::new(),
            java_classes: JavaClassIndex::default(),
        };
        index.scan(source_root)?;
        // Resolution needs the complete file set (extension/index guessing)
        // and the full package map, so it runs as a second pass.
        let java_classes = JavaClassIndex::new(&index.files, &index.packages);
        for (importer, imports) in &mut index.imports {
            let is_java = SourceLanguage::for_path(importer) == Some(SourceLanguage::Java);
            for resolved in imports {
                resolved.target = if is_java {
                    java_classes
                        .resolve(&resolved.record.specifier)
                        .map(PathBuf::from)
                } else {
                    resolve_module(importer, &resolved.record.specifier, &index.files)
                };
            }
        }
        index.java_classes = java_classes;
        Ok(index)
    }

    // Walk the project and parse each supported source file, staging the
    // raw records with `target: None` for the resolution pass above. When
    // `source_root` (project-relative, normalized) is set, only files under
    // that subtree are indexed — see [`Index::build_scoped`].
    fn scan(&mut self, source_root: Option<&Path>) -> JmoveResult<()> {
        // Sorted map: deterministic discovery order. Walking starts at the
        // scoped subtree when set, so the rest of the monorepo is not even
        // opened; paths stay root-relative because the prefix removed is
        // always `self.root`.
        let mut found: BTreeMap<PathBuf, SourceLanguage> = BTreeMap::new();
        let base = source_root.map_or(self.root.clone(), |scope| self.root.join(scope));
        for entry in WalkBuilder::new(base).require_git(false).build() {
            // Walker errors (unreadable dirs, etc.) simply skip the entry.
            let Ok(entry) = entry else { continue };
            if entry.path_is_symlink() || !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            let stripped = entry
                .path()
                .strip_prefix(&self.root)
                .unwrap_or(Path::new(""));
            let Some(rel) = normalize_rel_path(stripped) else {
                continue;
            };
            if let Some(lang) = SourceLanguage::for_path(&rel) {
                found.insert(rel, lang);
            }
        }
        // One frontend per language, created lazily as languages appear.
        let mut frontends: Vec<(SourceLanguage, Box<dyn Language>)> = Vec::new();
        for (rel, lang) in found {
            let Ok(text) = fs::read_to_string(self.root.join(&rel)) else {
                continue; // non-UTF-8 or vanished between scan and read
            };
            if !frontends.iter().any(|(l, _)| *l == lang) {
                frontends.push((lang, frontend_for(lang)));
            }
            let frontend = &mut frontends
                .iter_mut()
                .find(|(l, _)| *l == lang)
                .expect("frontend was just ensured")
                .1;
            let records = frontend.extract_imports(&text);
            self.files.add(rel.clone());
            if let Some(decl) = frontend.extract_package(&text) {
                self.packages.insert(rel.clone(), decl);
            }
            self.imports.insert(
                rel,
                records
                    .into_iter()
                    .map(|record| ResolvedImport {
                        record,
                        target: None,
                    })
                    .collect(),
            );
        }
        Ok(())
    }

    /// Reverse edge lookup: every indexed file that imports `target`.
    #[must_use]
    pub fn importers_of(&self, target: &Path) -> Vec<PathBuf> {
        let mut importers: Vec<PathBuf> = self
            .imports
            .iter()
            .filter(|(_, imports)| imports.iter().any(|r| r.target.as_deref() == Some(target)))
            .map(|(file, _)| file.clone())
            .collect();
        importers.sort();
        importers
    }
}
