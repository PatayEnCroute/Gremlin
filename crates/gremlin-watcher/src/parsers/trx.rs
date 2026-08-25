use super::{bounded_count, checked_metadata, ParseFailure, MAX_XML_DEPTH, MAX_XML_FILE_BYTES};
use crate::signals::{ParsedTestReport, ReportFramework};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::io::BufReader;
use std::path::Path;
use std::time::Duration;

pub(super) fn parse(path: &Path) -> Result<ParsedTestReport, ParseFailure> {
    let _ = checked_metadata(path, MAX_XML_FILE_BYTES)?;
    let file = std::fs::File::open(path)
        .map_err(|error| ParseFailure::Incomplete(format!("lecture impossible : {error}")))?;
    let mut reader = Reader::from_reader(BufReader::with_capacity(32 * 1024, file));
    let mut buffer = Vec::with_capacity(8 * 1024);
    let mut summary = None;
    let mut depth = 0_usize;
    let mut root_closed = false;

    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(element)) if element.name().as_ref() == b"Counters" => {
                if depth >= MAX_XML_DEPTH {
                    return Err(ParseFailure::Rejected(String::from(
                        "profondeur XML maximale dépassée",
                    )));
                }
                summary = Some(parse_counters(&element)?);
                depth = depth.saturating_add(1);
            }
            Ok(Event::Empty(element)) if element.name().as_ref() == b"Counters" => {
                summary = Some(parse_counters(&element)?);
            }
            Ok(Event::Start(_)) => {
                if depth >= MAX_XML_DEPTH {
                    return Err(ParseFailure::Rejected(String::from(
                        "profondeur XML maximale dépassée",
                    )));
                }
                depth = depth.saturating_add(1);
            }
            Ok(Event::End(_)) => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    root_closed = true;
                }
            }
            Ok(Event::DocType(_)) => {
                return Err(ParseFailure::Rejected(String::from(
                    "DOCTYPE interdit dans un rapport TRX",
                )));
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(ParseFailure::Incomplete(format!(
                    "TRX incomplet ou invalide : {error}"
                )));
            }
        }
        buffer.clear();
    }

    if !root_closed {
        return Err(ParseFailure::Incomplete(String::from(
            "rapport TRX non fermé",
        )));
    }
    summary.ok_or_else(|| {
        ParseFailure::Rejected(String::from("élément Counters absent du rapport TRX"))
    })
}

fn parse_counters(
    element: &quick_xml::events::BytesStart<'_>,
) -> Result<ParsedTestReport, ParseFailure> {
    let mut total = None;
    let mut passed = 0_u64;
    let mut failed = 0_u64;
    for attribute in element.attributes().with_checks(true) {
        let attribute = attribute
            .map_err(|error| ParseFailure::Rejected(format!("attribut TRX invalide : {error}")))?;
        let value = std::str::from_utf8(attribute.value.as_ref())
            .map_err(|error| ParseFailure::Rejected(error.to_string()))?
            .parse::<u64>()
            .map_err(|error| ParseFailure::Rejected(error.to_string()))?;
        match attribute.key.as_ref() {
            b"total" => total = Some(value),
            b"passed" => passed = value,
            b"failed" | b"error" | b"timeout" | b"aborted" => {
                failed = failed.saturating_add(value);
            }
            _ => {}
        }
    }
    let total =
        total.ok_or_else(|| ParseFailure::Rejected(String::from("compteur total TRX absent")))?;
    if passed.saturating_add(failed) > total {
        return Err(ParseFailure::Rejected(String::from(
            "compteurs TRX incohérents",
        )));
    }
    Ok(ParsedTestReport {
        framework: ReportFramework::Dotnet,
        passed: bounded_count(passed, "passed")?,
        failed: bounded_count(failed, "failed")?,
        skipped: bounded_count(total - passed - failed, "skipped")?,
        duration: Duration::ZERO,
    })
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::test_support::{write_file, TempDirGuard};

    #[test]
    fn test_trx_aggregates_terminal_outcomes() {
        let guard = TempDirGuard::new("trx_counts");
        let path = guard.path().join("result.trx");
        write_file(
            &path,
            r#"<TestRun><ResultSummary><Counters total="8" passed="5" failed="1" error="1" aborted="1"/></ResultSummary></TestRun>"#,
        );
        let summary = parse(&path).expect("rapport TRX");
        assert_eq!(summary.passed, 5);
        assert_eq!(summary.failed, 3);
        assert_eq!(summary.skipped, 0);
    }

    #[test]
    fn test_truncated_trx_is_incomplete_even_after_counters() {
        let guard = TempDirGuard::new("trx_truncated");
        let path = guard.path().join("result.trx");
        write_file(
            &path,
            r#"<TestRun><ResultSummary><Counters total="1" passed="1"/>"#,
        );
        assert!(matches!(parse(&path), Err(ParseFailure::Incomplete(_))));
    }
}
