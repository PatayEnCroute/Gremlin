use super::{
    bounded_count, checked_metadata, duration_from_secs, ParseFailure, MAX_XML_DEPTH,
    MAX_XML_FILE_BYTES,
};
use crate::signals::{ParsedTestReport, ReportFramework};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::io::BufReader;
use std::path::Path;

#[derive(Debug, Clone, Copy, Default)]
struct Counts {
    tests: u64,
    failed: u64,
    errors: u64,
    skipped: u64,
    duration_secs: f64,
}

impl Counts {
    fn add(&mut self, other: Self) {
        self.tests = self.tests.saturating_add(other.tests);
        self.failed = self.failed.saturating_add(other.failed);
        self.errors = self.errors.saturating_add(other.errors);
        self.skipped = self.skipped.saturating_add(other.skipped);
        self.duration_secs += other.duration_secs;
    }
}

pub(super) fn parse(
    path: &Path,
    framework: ReportFramework,
) -> Result<ParsedTestReport, ParseFailure> {
    let _ = checked_metadata(path, MAX_XML_FILE_BYTES)?;
    let file = std::fs::File::open(path)
        .map_err(|error| ParseFailure::Incomplete(format!("lecture impossible : {error}")))?;
    let mut reader = Reader::from_reader(BufReader::with_capacity(32 * 1024, file));
    reader.config_mut().trim_text(true);

    let mut buffer = Vec::with_capacity(8 * 1024);
    let mut depth = 0_usize;
    let mut root_is_collection = false;
    let mut root_counts: Option<Counts> = None;
    let mut direct_counts = Counts::default();
    let mut direct_seen = false;
    let mut root_closed = false;

    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(element)) => {
                if depth >= MAX_XML_DEPTH {
                    return Err(ParseFailure::Rejected(String::from(
                        "profondeur XML maximale dépassée",
                    )));
                }
                process_element(
                    &element,
                    depth,
                    &mut root_is_collection,
                    &mut root_counts,
                    &mut direct_counts,
                    &mut direct_seen,
                )?;
                depth = depth.saturating_add(1);
            }
            Ok(Event::Empty(element)) => {
                process_element(
                    &element,
                    depth,
                    &mut root_is_collection,
                    &mut root_counts,
                    &mut direct_counts,
                    &mut direct_seen,
                )?;
                if depth == 0 {
                    root_closed = true;
                }
            }
            Ok(Event::End(_)) => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    root_closed = true;
                }
            }
            Ok(Event::DocType(_)) => {
                return Err(ParseFailure::Rejected(String::from(
                    "DOCTYPE interdit dans un rapport XML",
                )));
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                let reason = format!("XML incomplet ou invalide : {error}");
                return if error.to_string().contains("Unexpected EOF") {
                    Err(ParseFailure::Incomplete(reason))
                } else {
                    Err(ParseFailure::Rejected(reason))
                };
            }
        }
        buffer.clear();
    }

    if !root_closed {
        return Err(ParseFailure::Incomplete(String::from(
            "rapport XML non fermé",
        )));
    }
    let counts = match (root_counts, direct_seen) {
        (Some(root), true) if root.is_coherent_with(direct_counts) => root,
        (_, true) => direct_counts,
        (Some(root), false) => root,
        (None, false) => {
            return Err(ParseFailure::Rejected(String::from(
                "aucun résumé JUnit exploitable",
            )))
        }
    };
    build_summary(counts, framework)
}

fn process_element(
    element: &BytesStart<'_>,
    depth: usize,
    root_is_collection: &mut bool,
    root_counts: &mut Option<Counts>,
    direct_counts: &mut Counts,
    direct_seen: &mut bool,
) -> Result<(), ParseFailure> {
    let binding = element.name();
    let name = binding.as_ref();
    if depth == 0 && name == b"testsuites" {
        *root_is_collection = true;
        *root_counts = parse_counts(element)?;
    } else if name == b"testsuite" {
        let counts = parse_counts(element)?;
        if depth == 0 {
            *root_counts = counts;
        } else if *root_is_collection && depth == 1 {
            if let Some(counts) = counts {
                direct_counts.add(counts);
                *direct_seen = true;
            }
        }
    }
    Ok(())
}

impl Counts {
    const fn is_coherent_with(self, other: Self) -> bool {
        self.tests == other.tests
            && self.failed == other.failed
            && self.errors == other.errors
            && self.skipped == other.skipped
    }
}

fn parse_counts(element: &BytesStart<'_>) -> Result<Option<Counts>, ParseFailure> {
    let mut counts = Counts::default();
    let mut has_tests = false;
    for attribute in element.attributes().with_checks(true) {
        let attribute = attribute
            .map_err(|error| ParseFailure::Rejected(format!("attribut XML invalide : {error}")))?;
        let value = std::str::from_utf8(attribute.value.as_ref())
            .map_err(|error| ParseFailure::Rejected(format!("attribut XML non UTF-8 : {error}")))?;
        match attribute.key.as_ref() {
            b"tests" => {
                counts.tests = parse_u64(value, "tests")?;
                has_tests = true;
            }
            b"failures" => counts.failed = parse_u64(value, "failures")?,
            b"errors" => counts.errors = parse_u64(value, "errors")?,
            b"skipped" | b"disabled" => counts.skipped = parse_u64(value, "skipped")?,
            b"time" => {
                counts.duration_secs = value.parse::<f64>().map_err(|error| {
                    ParseFailure::Rejected(format!("durée JUnit invalide : {error}"))
                })?;
            }
            _ => {}
        }
    }
    Ok(has_tests.then_some(counts))
}

fn parse_u64(value: &str, name: &str) -> Result<u64, ParseFailure> {
    value
        .parse::<u64>()
        .map_err(|error| ParseFailure::Rejected(format!("compteur {name} invalide : {error}")))
}

fn build_summary(
    counts: Counts,
    framework: ReportFramework,
) -> Result<ParsedTestReport, ParseFailure> {
    let failed = counts.failed.saturating_add(counts.errors);
    let accounted = failed.saturating_add(counts.skipped);
    if accounted > counts.tests {
        return Err(ParseFailure::Rejected(String::from(
            "compteurs JUnit incohérents",
        )));
    }
    Ok(ParsedTestReport {
        framework,
        passed: bounded_count(counts.tests - accounted, "passed")?,
        failed: bounded_count(failed, "failed")?,
        skipped: bounded_count(counts.skipped, "skipped")?,
        duration: duration_from_secs(counts.duration_secs)?,
    })
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::test_support::{write_file, TempDirGuard};

    #[test]
    fn test_nested_suites_are_not_double_counted() {
        let guard = TempDirGuard::new("junit_nested");
        let path = guard.path().join("junit.xml");
        write_file(
            &path,
            r#"<testsuites><testsuite tests="3" failures="1" skipped="1" time="1.5"><testsuite tests="3" failures="1" skipped="1"/></testsuite></testsuites>"#,
        );
        let summary = parse(&path, ReportFramework::Rust).expect("rapport");
        assert_eq!(summary.passed, 1);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.skipped, 1);
    }

    #[test]
    fn test_incoherent_root_falls_back_to_direct_suites() {
        let guard = TempDirGuard::new("junit_incoherent_root");
        let path = guard.path().join("junit.xml");
        write_file(
            &path,
            r#"<testsuites tests="99"><testsuite tests="4" failures="1"/></testsuites>"#,
        );
        let summary = parse(&path, ReportFramework::Python).expect("rapport");
        assert_eq!(summary.passed, 3);
        assert_eq!(summary.failed, 1);
    }

    #[test]
    fn test_doctype_and_truncated_xml_are_not_accepted() {
        let guard = TempDirGuard::new("junit_hostile");
        let path = guard.path().join("junit.xml");
        write_file(&path, "<!DOCTYPE testsuites><testsuites/>");
        assert!(matches!(
            parse(&path, ReportFramework::Generic),
            Err(ParseFailure::Rejected(_))
        ));
        write_file(&path, r#"<testsuite tests="1">"#);
        assert!(matches!(
            parse(&path, ReportFramework::Generic),
            Err(ParseFailure::Incomplete(_))
        ));
    }
}
