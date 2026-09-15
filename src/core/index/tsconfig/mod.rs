//! tsconfig `compilerOptions.paths`: the alias table for bare specifiers.
//!
//! Most real TS projects import through aliases (`@/utils/x`, `@cfg`).
//! Without the mapping those imports are invisible to the graph, so `mv`
//! would leave them pointing at the old location. Loading is best-effort by
//! design: no tsconfig, invalid JSON or an unreadable file simply mean "no
//! aliases", never an error. `extends` chains are not followed (v1).
//!
//! tsconfig is JSONC: comments and trailing commas are legal, so the text
//! is sanitized before `serde_json` sees it.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::core::index::FileSet;
use crate::core::{normalize_rel_path, rel_str};
use crate::parser::resolve::{MODULE_EXTS, resolve_base};

/// One `paths` entry. Star entries match by prefix, plain keys by equality.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    /// Text the specifier must start with (star stripped), e.g. `"@utils/"`.
    prefix: String,
    /// `false` for an exact key (`"@cfg"`).
    starred: bool,
    /// Project-relative directory (star) or module base (exact) the alias maps into.
    dir: PathBuf,
}

/// Alias table; empty is a perfectly normal state.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PathAliases {
    entries: Vec<Entry>,
}

impl PathAliases {
    /// Load `root/tsconfig.json`; anything unreadable yields no aliases.
    #[must_use]
    pub fn load(root: &Path) -> Self {
        let Ok(raw) = fs::read_to_string(root.join("tsconfig.json")) else {
            return Self::default();
        };
        Self::parse(&sanitize_jsonc(&raw))
    }

    fn parse(json: &str) -> Self {
        let Ok(value) = serde_json::from_str::<Value>(json) else {
            return Self::default();
        };
        let options = value.get("compilerOptions");
        let base = options
            .and_then(|o| o.get("baseUrl"))
            .and_then(Value::as_str)
            .and_then(|b| normalize_rel_path(Path::new(b)))
            .unwrap_or_default();
        let Some(paths) = options
            .and_then(|o| o.get("paths"))
            .and_then(Value::as_object)
        else {
            return Self::default();
        };
        let mut entries = Vec::new();
        for (key, targets) in paths {
            // Only the first candidate of a list is honoured (KISS).
            let Some(first) = targets
                .as_array()
                .and_then(|a| a.first())
                .and_then(Value::as_str)
            else {
                continue;
            };
            let starred = key.ends_with('*');
            let prefix = if starred {
                key.trim_end_matches('*').to_owned()
            } else {
                key.clone()
            };
            let stripped = first.trim_start_matches("./");
            let dir_src = if first.ends_with("/*") {
                stripped.trim_end_matches("/*")
            } else {
                stripped
            };
            let dir = normalize_rel_path(&base.join(dir_src));
            let Some(dir) = dir.filter(|d| !d.as_os_str().is_empty()) else {
                continue;
            };
            entries.push(Entry {
                prefix,
                starred,
                dir,
            });
        }
        // Longest alias prefix wins when several match (`@a/b/*` over `@a/*`).
        entries.sort_by(|a, b| b.prefix.cmp(&a.prefix));
        Self { entries }
    }

    /// Resolve a bare (non-`.`-starting) specifier to an indexed file.
    #[must_use]
    pub fn resolve(&self, specifier: &str, files: &FileSet) -> Option<PathBuf> {
        let entry = self.find(specifier)?;
        let base = if entry.starred {
            entry.dir.join(&specifier[entry.prefix.len()..])
        } else {
            entry.dir.clone()
        };
        resolve_base(&normalize_rel_path(&base)?, files)
    }

    /// Re-express `target` through the same alias `specifier` used before.
    /// Star entries map morphologically (the plan runs before the file
    /// exists at its destination, so no file-set check is possible): the
    /// target must sit inside the alias directory and shed its module
    /// extension the same way `resolve` would re-add it. `None` means
    /// "fall back to a relative specifier".
    #[must_use]
    pub fn remap(&self, specifier: &str, target: &Path, files: &FileSet) -> Option<String> {
        let entry = self.find(specifier)?;
        if !entry.starred {
            // Exact key: it only keeps meaning while it maps to this file —
            // after the move the old mapping points elsewhere, so this is
            // normally `None` and the caller rewrites relatively.
            return (self.resolve(specifier, files)? == target).then(|| specifier.to_owned());
        }
        let rel = target.strip_prefix(&entry.dir).ok()?;
        let text = rel_str(rel);
        let stem = MODULE_EXTS
            .iter()
            .find(|ext| text.ends_with(**ext))
            .and_then(|ext| Some(text.strip_suffix(*ext)?.to_owned()))
            .unwrap_or(text);
        Some(format!("{}{}", entry.prefix, stem))
    }

    fn find(&self, specifier: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| {
            if e.starred {
                specifier.starts_with(&e.prefix) && specifier.len() > e.prefix.len()
            } else {
                specifier == e.prefix
            }
        })
    }
}

/// Strip `//` + `/* */` comments and trailing commas from JSONC text.
/// String awareness keeps `"http://x"` and escaped quotes safe.
#[must_use]
pub(crate) fn sanitize_jsonc(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    let mut in_string = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_string {
            out.push(b as char);
            if b == b'\\' && i + 1 < bytes.len() {
                out.push(bytes[i + 1] as char);
                i += 2;
                continue;
            }
            if b == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        match b {
            b'"' => {
                in_string = true;
                out.push('"');
                i += 1;
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                i = bytes[i..]
                    .iter()
                    .position(|c| *c == b'\n')
                    .map_or(bytes.len(), |p| i + p);
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let end = bytes[i + 2..]
                    .windows(2)
                    .position(|w| w == b"*/")
                    .map_or(bytes.len() - 2, |p| i + 2 + p + 2);
                i = end;
            }
            b',' => {
                // Drop only if the next non-space token closes a container.
                let rest = &bytes[i + 1..];
                match rest.iter().find(|c| !c.is_ascii_whitespace()) {
                    Some(b'}') | Some(b']') => i += 1,
                    _ => {
                        out.push(',');
                        i += 1;
                    }
                }
            }
            _ => {
                out.push(b as char);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
