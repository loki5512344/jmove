use super::{PathAliases, sanitize_jsonc};
use crate::core::index::FileSet;
use std::path::{Path, PathBuf};

fn files(paths: &[&str]) -> FileSet {
    let mut set = FileSet::default();
    for p in paths {
        set.add(PathBuf::from(p));
    }
    set
}

fn aliases(json: &str) -> PathAliases {
    PathAliases::parse(&sanitize_jsonc(json))
}

const TSCONFIG: &str = r#"{
  // comment with a brace }
  "compilerOptions": {
"baseUrl": "./",
"paths": {
  "@/*": ["src/*"],
  "@utils/*": ["src/shared/utils/*"],
  "@cfg": ["src/config.ts"],
},
  },
  "include": ["src/**/*"],
}"#;

#[test]
fn jsonc_comments_and_trailing_commas_parse() {
    let a = aliases(TSCONFIG);
    assert_eq!(a.entries.len(), 3);
}

#[test]
fn resolution_prefers_the_longest_matching_prefix() {
    let set = files(&[
        "src/shared/utils/str.ts",
        "src/utils/str.ts",
        "src/config.ts",
    ]);
    let a = aliases(TSCONFIG);
    assert_eq!(
        a.resolve("@utils/str", &set).as_deref(),
        Some(Path::new("src/shared/utils/str.ts"))
    );
    assert_eq!(
        a.resolve("@/utils/str", &set).as_deref(),
        Some(Path::new("src/utils/str.ts"))
    );
    assert_eq!(
        a.resolve("@cfg", &set).as_deref(),
        Some(Path::new("src/config.ts"))
    );
    assert_eq!(a.resolve("react", &set), None);
}

#[test]
fn remap_keeps_alias_shape_inside_and_falls_back_outside() {
    let set = files(&["src/utils/str.ts", "src/deep/str.ts", "src/config.ts"]);
    let a = aliases(r#"{"compilerOptions": {"paths": {"@u/*": ["src/utils/*"]}}}"#);
    // Destination need not exist yet: the plan runs before the move.
    assert_eq!(
        a.remap("@u/str", Path::new("src/utils/other.ts"), &set)
            .as_deref(),
        Some("@u/other")
    );
    // Target outside the alias tree: caller must fall back to relative.
    assert!(a.remap("@u/str", Path::new("lib/other.ts"), &set).is_none());
}

#[test]
fn string_contents_survive_sanitizing() {
    let json = r#"{"url": "http://x.com//y", "a": [1, 2,], /* c */ "b": "/*n*/"}"#;
    let cleaned = sanitize_jsonc(json);
    let value: serde_json::Value = serde_json::from_str(&cleaned).unwrap();
    assert_eq!(value["url"], "http://x.com//y");
    assert_eq!(value["b"], "/*n*/");
}
