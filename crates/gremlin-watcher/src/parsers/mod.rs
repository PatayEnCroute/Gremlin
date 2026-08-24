//! Parseurs bornés des rapports d'outillage pris en charge.

mod gremlin_json;
mod jest;
mod junit;
mod trx;

use crate::config::{ToolingFrameworkHint, ToolingReportFormat, ToolingSourceConfig};
use crate::signals::{ParsedBuildReport, ParsedTestReport, ReportFramework};
use std::path::Path;

pub(crate) const MAX_XML_FILE_BYTES: u64 = 8 * 1024 * 1024;
pub(crate) const MAX_JSON_FILE_BYTES: u64 = 1024 * 1024;
pub(crate) const MAX_PARSED_TESTS: u64 = 10_000_000;
pub(crate) const MAX_REPORT_DURATION_SECS: f64 = 7.0 * 24.0 * 60.0 * 60.0;
pub(crate) const MAX_XML_DEPTH: usize = 128;

#[derive(Debug)]
pub(crate) enum ParsedReport {
    Test {
        summary: ParsedTestReport,
        run_id: Option<String>,
    },
    Build {
        summary: ParsedBuildReport,
        run_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ParseFailure {
    Incomplete(String),
    Rejected(String),
}

impl ParseFailure {
    pub(crate) fn reason(&self) -> &str {
        match self {
            Self::Incomplete(reason) | Self::Rejected(reason) => reason,
        }
    }

    pub(crate) const fn is_incomplete(&self) -> bool {
        matches!(self, Self::Incomplete(_))
    }
}

pub(crate) fn parse_report(
    path: &Path,
    source: &ToolingSourceConfig,
    inferred_framework: ReportFramework,
) -> Result<ParsedReport, ParseFailure> {
    let framework = framework_from_hint(source.framework, inferred_framework);
    let format = if source.format == ToolingReportFormat::Auto {
        infer_format(path)
    } else {
        source.format
    };

    match format {
        ToolingReportFormat::Junit => {
            junit::parse(path, framework).map(|summary| ParsedReport::Test {
                summary,
                run_id: None,
            })
        }
        ToolingReportFormat::Trx => trx::parse(path).map(|summary| ParsedReport::Test {
            summary,
            run_id: None,
        }),
        ToolingReportFormat::JestJson => jest::parse(path).map(|summary| ParsedReport::Test {
            summary,
            run_id: None,
        }),
        ToolingReportFormat::GremlinJson => gremlin_json::parse(path),
        ToolingReportFormat::Auto => Err(ParseFailure::Rejected(String::from(
            "format de rapport impossible à déterminer",
        ))),
    }
}

fn infer_format(path: &Path) -> ToolingReportFormat {
    match path
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("trx") => ToolingReportFormat::Trx,
        Some("xml") => ToolingReportFormat::Junit,
        Some("json") if path.components().any(|part| part.as_os_str() == ".gremlin") => {
            ToolingReportFormat::GremlinJson
        }
        Some("json") => ToolingReportFormat::JestJson,
        _ => ToolingReportFormat::Auto,
    }
}

fn framework_from_hint(hint: ToolingFrameworkHint, inferred: ReportFramework) -> ReportFramework {
    match hint {
        ToolingFrameworkHint::Auto => inferred,
        ToolingFrameworkHint::Rust => ReportFramework::Rust,
        ToolingFrameworkHint::JavaScript => ReportFramework::JavaScript,
        ToolingFrameworkHint::Python => ReportFramework::Python,
        ToolingFrameworkHint::Go => ReportFramework::Go,
        ToolingFrameworkHint::Dotnet => ReportFramework::Dotnet,
        ToolingFrameworkHint::Generic => ReportFramework::Generic,
    }
}

pub(crate) fn checked_metadata(
    path: &Path,
    maximum: u64,
) -> Result<std::fs::Metadata, ParseFailure> {
    let metadata = std::fs::metadata(path).map_err(|error| {
        ParseFailure::Incomplete(format!("métadonnées indisponibles : {error}"))
    })?;
    if !metadata.is_file() {
        return Err(ParseFailure::Rejected(String::from(
            "la source de rapport n'est pas un fichier ordinaire",
        )));
    }
    if metadata.len() > maximum {
        return Err(ParseFailure::Rejected(format!(
            "rapport trop grand : {} octets (maximum {maximum})",
            metadata.len()
        )));
    }
    Ok(metadata)
}

pub(crate) fn bounded_count(value: u64, name: &str) -> Result<u32, ParseFailure> {
    if value > MAX_PARSED_TESTS {
        return Err(ParseFailure::Rejected(format!(
            "compteur {name} hors borne : {value}"
        )));
    }
    u32::try_from(value)
        .map_err(|_| ParseFailure::Rejected(format!("compteur {name} non représentable")))
}

pub(crate) fn duration_from_secs(value: f64) -> Result<std::time::Duration, ParseFailure> {
    if !value.is_finite() || value.is_sign_negative() || value > MAX_REPORT_DURATION_SECS {
        return Err(ParseFailure::Rejected(format!(
            "durée de rapport invalide : {value}"
        )));
    }
    Ok(std::time::Duration::from_secs_f64(value))
}
