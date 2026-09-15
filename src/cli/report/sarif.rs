//! SARIF 2.1.0 emission: single run, single tool driver, plain results.
//!
//! Only the subset consumed by GitHub code scanning / CodeQL upload /
//! VSCode is produced: `ruleId`, `level`, one physical `location` with a
//! start line, and a `properties.autoFixable` flag. `uriBaseId` is
//! `%SRCROOT%` so artifact URIs stay project-relative.

use serde::Serialize;

use super::Violation;

const SCHEMA: &str = "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/master/Schemata/sarif-schema-2.1.0.json";

/// Render `violations` as a pretty-printed SARIF document.
#[must_use]
pub fn build(violations: &[Violation]) -> String {
    let results = violations
        .iter()
        .map(|v| SarifResult {
            rule_id: v.rule,
            level: level(v.severity),
            message: Text {
                text: v.message.clone(),
            },
            locations: vec![Location {
                physical_location: Physical {
                    artifact_location: Artifact {
                        uri: v.file.clone(),
                        uri_base_id: "%SRCROOT%",
                    },
                    region: Region { start_line: v.line },
                },
            }],
            properties: Props {
                auto_fixable: v.fixable,
            },
        })
        .collect();
    let doc = Sarif {
        schema: SCHEMA,
        version: "2.1.0",
        runs: vec![Run {
            tool: Tool {
                driver: Driver {
                    name: "jmove",
                    version: env!("CARGO_PKG_VERSION"),
                    rules: driver_rules(violations),
                },
            },
            results,
        }],
    };
    serde_json::to_string_pretty(&doc).expect("sarif shapes always serialize")
}

/// SARIF `level`: the spec's vocabulary, mapped from jmove severities.
fn level(severity: &str) -> &'static str {
    match severity {
        "warning" => "warning",
        "info" => "note",
        _ => "error",
    }
}

// The driver rule registry: each distinct rule once, sorted, described.
fn driver_rules(violations: &[Violation]) -> Vec<Rule> {
    let mut ids: Vec<&'static str> = Vec::new();
    for v in violations {
        if !ids.contains(&v.rule) {
            ids.push(v.rule);
        }
    }
    ids.sort_unstable();
    ids.into_iter()
        .map(|id| Rule {
            id,
            short_description: Text {
                text: description(id).to_owned(),
            },
        })
        .collect()
}

/// Stable one-line rule descriptions (`shortDescription.text`).
fn description(id: &str) -> &str {
    match id {
        "broken-import" => "Relative import specifier cannot be resolved",
        "java/class-name-mismatch" => "File name must match the public Java type",
        "java/unused-import" | "ts/unused-import" => "Import is never referenced",
        "java/missing-import" => "Referenced type has no import",
        "java/import-order" => "Imports violate the configured order",
        other => other,
    }
}

#[derive(Serialize)]
struct Sarif {
    #[serde(rename = "$schema")]
    schema: &'static str,
    version: &'static str,
    runs: Vec<Run>,
}

#[derive(Serialize)]
struct Tool {
    driver: Driver,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Driver {
    name: &'static str,
    version: &'static str,
    rules: Vec<Rule>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Rule {
    id: &'static str,
    short_description: Text,
}

#[derive(Serialize)]
struct Text {
    text: String,
}

#[derive(Serialize)]
struct Run {
    tool: Tool,
    results: Vec<SarifResult>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SarifResult {
    rule_id: &'static str,
    level: &'static str,
    message: Text,
    locations: Vec<Location>,
    properties: Props,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Location {
    physical_location: Physical,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Physical {
    artifact_location: Artifact,
    region: Region,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Artifact {
    uri: String,
    uri_base_id: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Region {
    start_line: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Props {
    auto_fixable: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;

    fn violation(rule: &'static str, severity: &'static str, file: &str, line: usize) -> Violation {
        Violation {
            rule,
            severity,
            message: format!("{rule} at {file}:{line}"),
            file: file.to_owned(),
            line,
            fixable: severity != "error",
        }
    }

    fn parse(violations: &[Violation]) -> serde_json::Value {
        serde_json::from_str(&build(violations)).expect("valid json")
    }

    #[test]
    fn envelope_and_result_shapes_follow_the_spec() {
        let doc = parse(&[
            violation("java/unused-import", "warning", "a.java", 3),
            violation("java/import-order", "info", "a.java", 9),
        ]);
        assert_eq!(doc["version"], "2.1.0");
        assert!(doc["$schema"].is_string());
        let run = &doc["runs"][0];
        assert_eq!(run["tool"]["driver"]["name"], "jmove");
        assert_eq!(run["results"][0]["ruleId"], "java/unused-import");
        assert_eq!(run["results"][0]["level"], "warning");
        let location = &run["results"][0]["locations"][0]["physicalLocation"];
        assert_eq!(location["artifactLocation"]["uri"], "a.java");
        assert_eq!(location["artifactLocation"]["uriBaseId"], "%SRCROOT%");
        assert_eq!(location["region"]["startLine"], 3);
        assert_eq!(run["results"][0]["properties"]["autoFixable"], true);
        assert_eq!(run["results"][1]["level"], "note");
    }

    #[test]
    fn driver_lists_each_rule_once_sorted_with_descriptions() {
        let doc = parse(&[
            violation("java/unused-import", "warning", "a.java", 1),
            violation("broken-import", "error", "b.ts", 2),
            violation("java/unused-import", "warning", "c.java", 3),
        ]);
        let ids: Vec<&str> = doc["runs"][0]["tool"]["driver"]["rules"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["broken-import", "java/unused-import"]);
        assert_eq!(doc["runs"][0]["results"][1]["level"], "error");
        assert!(
            doc["runs"][0]["tool"]["driver"]["rules"][0]["shortDescription"]["text"]
                .as_str()
                .unwrap()
                .starts_with("Relative")
        );
    }

    #[test]
    fn every_shipped_rule_id_has_a_description() {
        for id in parser::rule_ids() {
            assert_ne!(description(id), *id, "missing description for {id}");
        }
        assert!(!description("broken-import").starts_with("broken-import"));
    }
}
