//! End-to-end tests for tsconfig `paths` aliases: aliased imports take
//! part in `mv` and keep their alias shape inside the mapped tree.

mod common;

use common::{copy_fixture, in_root, jmove, read};
use predicates::prelude::*;

const APP: &str = "src/app.ts";

#[test]
fn aliased_imports_follow_the_move_and_keep_the_alias_shape() {
    let tmp = copy_fixture("typescript", "aliased");
    jmove(&tmp, &["check"]).success(); // "@utils/str" resolves: not "broken"
    jmove(&tmp, &["mv", "src/utils/str.ts", "src/utils/text.ts"])
        .success()
        .stdout(predicate::str::contains("updated 1 import"));
    let app = read(&in_root(tmp.path(), APP));
    assert!(app.contains("from \"@utils/text\""), "{app}");
    assert!(!app.contains("./utils/text"), "{app}");
    // Moving out of the alias tree falls back to a relative specifier.
    jmove(&tmp, &["mv", "src/utils/text.ts", "src/core/text.ts"]).success();
    let app = read(&in_root(tmp.path(), APP));
    assert!(app.contains("from \"./core/text\""), "{app}");
    jmove(&tmp, &["check"]).success();
}

#[test]
fn exact_alias_key_falls_back_when_it_no_longer_matches() {
    let tmp = copy_fixture("typescript", "aliased");
    jmove(&tmp, &["mv", "src/config.ts", "src/settings.ts"]).success();
    let app = read(&in_root(tmp.path(), APP));
    // "@cfg" would now resolve to nothing: the rewrite must be relative.
    assert!(app.contains("from \"./settings\""), "{app}");
    assert!(!app.contains("@cfg"), "{app}");
    jmove(&tmp, &["check"]).success();
}
