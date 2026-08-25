use super::{
    bounded_count, checked_metadata, ParseFailure, ParsedReport, MAX_JSON_FILE_BYTES,
    MAX_REPORT_DURATION_SECS,
};
use crate::signals::{ParsedBuildReport, ParsedTestReport, ReportBuildTool, ReportFramework};
use serde::Deserialize;
use std::path::Path;
use std::time::Duration;

const MAX_RUN_ID_CHARS: usize = 128;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Test,
    Build,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Outcome {
    Passed,
    Failed,
}

#[derive(Debug, Deserialize)]
struct Contract {
    schema_version: u32,
    run_id: String,
    kind: Kind,
    tool: String,
    outcome: Outcome,
    #[serde(default)]
    passed: u64,
    #[serde(default)]
    failed: u64,
    #[serde(default)]
    skipped: u64,
    #[serde(default)]
    duration_ms: u64,
}

pub(super) fn parse(path: &Path) -> Result<ParsedReport, ParseFailure> {
    let _ = checked_metadata(path, MAX_JSON_FILE_BYTES)?;
    let bytes = std::fs::read(path)
        .map_err(|error| ParseFailure::Incomplete(format!("lecture impossible : {error}")))?;
    let report: Contract = serde_json::from_slice(&bytes).map_err(|error| {
        if error.is_eof() {
            ParseFailure::Incomplete(format!("contrat JSON incomplet : {error}"))
        } else {
            ParseFailure::Rejected(format!("contrat JSON invalide : {error}"))
        }
    })?;
    if report.schema_version != 1 {
        return Err(ParseFailure::Rejected(format!(
            "version de contrat non prise en charge : {}",
            report.schema_version
        )));
    }
    if report.run_id.trim().is_empty() || report.run_id.chars().count() > MAX_RUN_ID_CHARS {
        return Err(ParseFailure::Rejected(String::from("run_id invalide")));
    }
    if report.duration_ms > (MAX_REPORT_DURATION_SECS * 1_000.0) as u64 {
        return Err(ParseFailure::Rejected(String::from(
            "durée du contrat hors borne",
        )));
    }
    let duration = Duration::from_millis(report.duration_ms);

    match report.kind {
        Kind::Test => {
            let outcome_is_valid = match report.outcome {
                Outcome::Passed => report.failed == 0 && report.passed > 0,
                Outcome::Failed => report.failed > 0,
            };
            if !outcome_is_valid {
                return Err(ParseFailure::Rejected(String::from(
                    "outcome incohérent avec les compteurs de tests",
                )));
            }
            Ok(ParsedReport::Test {
                summary: ParsedTestReport {
                    framework: framework(&report.tool),
                    passed: bounded_count(report.passed, "passed")?,
                    failed: bounded_count(report.failed, "failed")?,
                    skipped: bounded_count(report.skipped, "skipped")?,
                    duration,
                },
                run_id: Some(report.run_id),
            })
        }
        Kind::Build => Ok(ParsedReport::Build {
            summary: ParsedBuildReport {
                tool: build_tool(&report.tool),
                success: matches!(report.outcome, Outcome::Passed),
                duration,
            },
            run_id: report.run_id,
        }),
    }
}

fn framework(tool: &str) -> ReportFramework {
    match tool.to_ascii_lowercase().as_str() {
        "cargo" | "rust" => ReportFramework::Rust,
        "npm" | "jest" | "vitest" | "mocha" => ReportFramework::JavaScript,
        "python" | "pytest" => ReportFramework::Python,
        "go" => ReportFramework::Go,
        "dotnet" | ".net" => ReportFramework::Dotnet,
        _ => ReportFramework::Generic,
    }
}

fn build_tool(tool: &str) -> ReportBuildTool {
    match tool.to_ascii_lowercase().as_str() {
        "cargo" | "rust" => ReportBuildTool::Cargo,
        "npm" => ReportBuildTool::Npm,
        "webpack" | "vite" => ReportBuildTool::WebpackOrVite,
        "python" => ReportBuildTool::Python,
        "go" => ReportBuildTool::Go,
        "dotnet" | ".net" => ReportBuildTool::Dotnet,
        _ => ReportBuildTool::Generic,
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::test_support::{write_file, TempDirGuard};

    #[test]
    fn test_contract_v1_supports_tests_and_builds() {
        let guard = TempDirGuard::new("gremlin_contract");
        let path = guard.path().join("result.json");
        write_file(
            &path,
            r#"{"schema_version":1,"run_id":"test-1","kind":"test","tool":"pytest","outcome":"failed","passed":3,"failed":1,"duration_ms":50}"#,
        );
        assert!(matches!(
            parse(&path).expect("contrat test"),
            ParsedReport::Test { summary, .. }
                if summary.framework == ReportFramework::Python && summary.failed == 1
        ));

        write_file(
            &path,
            r#"{"schema_version":1,"run_id":"build-1","kind":"build","tool":"vite","outcome":"passed","duration_ms":75}"#,
        );
        assert!(matches!(
            parse(&path).expect("contrat build"),
            ParsedReport::Build { summary, .. }
                if summary.tool == ReportBuildTool::WebpackOrVite && summary.success
        ));
    }

    #[test]
    fn test_future_contract_and_hostile_values_are_rejected() {
        let guard = TempDirGuard::new("gremlin_contract_hostile");
        let path = guard.path().join("result.json");
        write_file(
            &path,
            r#"{"schema_version":2,"run_id":"future","kind":"build","tool":"cargo","outcome":"passed"}"#,
        );
        assert!(matches!(parse(&path), Err(ParseFailure::Rejected(_))));

        let long_id = "x".repeat(MAX_RUN_ID_CHARS + 1);
        write_file(
            &path,
            &format!(
                r#"{{"schema_version":1,"run_id":"{long_id}","kind":"build","tool":"cargo","outcome":"passed"}}"#
            ),
        );
        assert!(matches!(parse(&path), Err(ParseFailure::Rejected(_))));
    }
}
