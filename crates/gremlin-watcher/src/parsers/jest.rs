use super::{bounded_count, checked_metadata, ParseFailure, MAX_JSON_FILE_BYTES};
use crate::signals::{ParsedTestReport, ReportFramework};
use serde::Deserialize;
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Deserialize)]
struct JestSummary {
    #[serde(rename = "numPassedTests")]
    passed: u64,
    #[serde(rename = "numFailedTests")]
    failed: u64,
    #[serde(default)]
    #[serde(rename = "numPendingTests")]
    pending: u64,
}

pub(super) fn parse(path: &Path) -> Result<ParsedTestReport, ParseFailure> {
    let _ = checked_metadata(path, MAX_JSON_FILE_BYTES)?;
    let bytes = std::fs::read(path)
        .map_err(|error| ParseFailure::Incomplete(format!("lecture impossible : {error}")))?;
    let report: JestSummary = serde_json::from_slice(&bytes).map_err(|error| {
        if error.is_eof() {
            ParseFailure::Incomplete(format!("JSON Jest incomplet : {error}"))
        } else {
            ParseFailure::Rejected(format!("JSON Jest invalide : {error}"))
        }
    })?;

    Ok(ParsedTestReport {
        framework: ReportFramework::JavaScript,
        passed: bounded_count(report.passed, "passed")?,
        failed: bounded_count(report.failed, "failed")?,
        skipped: bounded_count(report.pending, "skipped")?,
        duration: Duration::ZERO,
    })
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::test_support::{write_file, TempDirGuard};

    #[test]
    fn test_jest_summary_ignores_detailed_arrays() {
        let guard = TempDirGuard::new("jest_summary");
        let path = guard.path().join("jest.json");
        write_file(
            &path,
            r#"{"testResults":[{"assertionResults":[{"title":"x"}]}],"numPendingTests":2,"numFailedTests":1,"numPassedTests":7}"#,
        );
        let summary = parse(&path).expect("rapport Jest");
        assert_eq!(summary.passed, 7);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.skipped, 2);
    }

    #[test]
    fn test_truncated_jest_is_incomplete() {
        let guard = TempDirGuard::new("jest_truncated");
        let path = guard.path().join("jest.json");
        write_file(&path, r#"{"numPassedTests":1"#);
        assert!(matches!(parse(&path), Err(ParseFailure::Incomplete(_))));
    }
}
