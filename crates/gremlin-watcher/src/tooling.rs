//! Pipeline borné de stabilisation et de parsing des rapports d'outillage.

use crate::config::{ToolingReportFormat, ToolingSourceConfig, WatcherConfig};
use crate::git_parser::GitRefParser;
use crate::parsers::{parse_report, ParseFailure, ParsedReport, MAX_XML_FILE_BYTES};
use crate::signals::{DevSignal, ReportFramework, WatcherStatus};
use crossbeam_channel::{Receiver, Select, Sender, TrySendError};
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};
use walkdir::WalkDir;

const PARSE_JOB_CAPACITY: usize = 32;
const PARSE_RESULT_CAPACITY: usize = 64;
const MAX_PENDING_REPORTS: usize = 256;
const MAX_BASELINE_REPORTS_PER_REPO: usize = 64;
const MAX_PARSE_RETRIES: u8 = 3;
const MAX_RECENT_RUN_IDS: usize = 256;
const MAX_FINGERPRINTS: usize = 4_096;

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileFingerprint {
    len: u64,
    modified: Option<SystemTime>,
    content_hash: u64,
}

#[derive(Debug, Clone)]
struct ParseJob {
    repo_root: PathBuf,
    repo_name: String,
    path: PathBuf,
    source: ToolingSourceConfig,
    framework: ReportFramework,
    attempts: u8,
}

#[derive(Debug)]
pub struct ParseResult {
    job: ParseJob,
    parsed: Result<(ParsedReport, FileFingerprint), ParseFailure>,
}

#[derive(Debug)]
enum ParserControl {
    Shutdown,
}

#[derive(Debug, Clone)]
struct PendingReport {
    repo_root: PathBuf,
    repo_name: String,
    path: PathBuf,
    source: ToolingSourceConfig,
    framework: ReportFramework,
    last_seen: Instant,
    attempts: u8,
}

pub enum ToolingEvent {
    Signal(DevSignal),
    Status(WatcherStatus),
}

/// État du pipeline, détenu exclusivement par le `WatcherWorker`.
pub struct ToolingPipeline {
    enabled: bool,
    sources: Vec<ToolingSourceConfig>,
    debounce: Duration,
    pending: HashMap<PathBuf, PendingReport>,
    fingerprints: HashMap<PathBuf, FileFingerprint>,
    recent_run_ids: VecDeque<(PathBuf, String)>,
    job_tx: Sender<ParseJob>,
    result_rx: Receiver<ParseResult>,
    control_tx: Sender<ParserControl>,
    parser_handle: Option<JoinHandle<()>>,
}

impl ToolingPipeline {
    pub(crate) fn new(config: &WatcherConfig) -> std::io::Result<Self> {
        let (job_tx, job_rx) = crossbeam_channel::bounded(PARSE_JOB_CAPACITY);
        let (result_tx, result_rx) = crossbeam_channel::bounded(PARSE_RESULT_CAPACITY);
        let (control_tx, control_rx) = crossbeam_channel::bounded(1);
        let parser_handle = std::thread::Builder::new()
            .name(String::from("gremlin-report-parser"))
            .spawn(move || parser_loop(&job_rx, &result_tx, &control_rx))?;

        Ok(Self {
            enabled: config.tooling_enabled,
            sources: config.tooling_sources.clone(),
            debounce: config.tooling_debounce_duration(),
            pending: HashMap::new(),
            fingerprints: HashMap::new(),
            recent_run_ids: VecDeque::new(),
            job_tx,
            result_rx,
            control_tx,
            parser_handle: Some(parser_handle),
        })
    }

    pub(crate) const fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.pending.clear();
            self.fingerprints.clear();
            self.recent_run_ids.clear();
        }
    }

    pub(crate) fn sources(&self) -> &[ToolingSourceConfig] {
        &self.sources
    }

    pub(crate) fn result_receiver(&self) -> &Receiver<ParseResult> {
        &self.result_rx
    }

    pub(crate) fn seed_repo(&mut self, repo_root: &Path) {
        if !self.enabled {
            return;
        }
        let mut seeded = 0_usize;
        let sources = self.sources.clone();
        for source in &sources {
            let path = repo_root.join(&source.relative_path);
            if path.is_file() {
                if let Ok(fingerprint) = fingerprint(&path) {
                    self.remember_fingerprint(path, fingerprint);
                    seeded += 1;
                }
                continue;
            }
            if !path.is_dir() {
                continue;
            }
            for entry in WalkDir::new(&path)
                .max_depth(4)
                .follow_links(false)
                .into_iter()
                .filter_map(Result::ok)
            {
                if seeded >= MAX_BASELINE_REPORTS_PER_REPO {
                    return;
                }
                let candidate = entry.path();
                if candidate.is_file() && candidate_matches(candidate, source.format) {
                    if let Ok(fingerprint) = fingerprint(candidate) {
                        self.remember_fingerprint(candidate.to_path_buf(), fingerprint);
                        seeded += 1;
                    }
                }
            }
        }
    }

    pub(crate) fn remove_repo(&mut self, repo_root: &Path) {
        self.pending
            .retain(|_, pending| pending.repo_root != repo_root);
        self.fingerprints
            .retain(|path, _| !path.starts_with(repo_root));
        self.recent_run_ids
            .retain(|(repo, _)| repo.as_path() != repo_root);
    }

    pub(crate) fn forget_path(&mut self, path: &Path) {
        let _ = self.pending.remove(path);
        let _ = self.fingerprints.remove(path);
    }

    pub(crate) fn record_path(&mut self, repo_root: &Path, path: &Path) -> Option<WatcherStatus> {
        if !self.enabled || !path.is_file() {
            return None;
        }
        let source = self.matching_source(repo_root, path)?.clone();
        if !candidate_matches(path, source.format) {
            return None;
        }
        if !self.pending.contains_key(path) && self.pending.len() >= MAX_PENDING_REPORTS {
            return Some(WatcherStatus::ReportRejected {
                path: path.to_path_buf(),
                reason: String::from("capacité des rapports en attente atteinte"),
            });
        }

        let pending = PendingReport {
            repo_root: repo_root.to_path_buf(),
            repo_name: GitRefParser::extract_repo_name(repo_root),
            path: path.to_path_buf(),
            source,
            framework: infer_framework(repo_root, path),
            last_seen: Instant::now(),
            attempts: 0,
        };
        let _ = self.pending.insert(path.to_path_buf(), pending);
        None
    }

    pub(crate) fn record_directory(
        &mut self,
        repo_root: &Path,
        directory: &Path,
    ) -> Vec<WatcherStatus> {
        let mut statuses = Vec::new();
        for entry in WalkDir::new(directory)
            .max_depth(4)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
            .take(MAX_BASELINE_REPORTS_PER_REPO)
        {
            if let Some(status) = self.record_path(repo_root, entry.path()) {
                statuses.push(status);
            }
        }
        statuses
    }

    pub(crate) fn submit_ready(&mut self) {
        let now = Instant::now();
        let ready: Vec<PathBuf> = self
            .pending
            .iter()
            .filter(|(_, pending)| now.duration_since(pending.last_seen) >= self.debounce)
            .map(|(path, _)| path.clone())
            .collect();

        for path in ready {
            let Some(pending) = self.pending.remove(&path) else {
                continue;
            };
            let job = ParseJob {
                repo_root: pending.repo_root,
                repo_name: pending.repo_name,
                path: pending.path,
                source: pending.source,
                framework: pending.framework,
                attempts: pending.attempts,
            };
            if let Err(TrySendError::Full(job)) = self.job_tx.try_send(job) {
                self.requeue(job);
            }
        }
    }

    pub(crate) fn drain_results(&mut self) -> Vec<ToolingEvent> {
        let mut events = Vec::new();
        while let Ok(result) = self.result_rx.try_recv() {
            self.handle_result(result, &mut events);
        }
        events
    }

    pub(crate) fn time_until_next_ready(&self) -> Option<Duration> {
        let now = Instant::now();
        self.pending
            .values()
            .map(|pending| {
                self.debounce
                    .saturating_sub(now.duration_since(pending.last_seen))
            })
            .min()
    }

    fn matching_source(&self, repo_root: &Path, path: &Path) -> Option<&ToolingSourceConfig> {
        self.sources.iter().find(|source| {
            let configured = repo_root.join(&source.relative_path);
            if configured.extension().is_some() {
                path == configured
            } else {
                path.starts_with(configured)
            }
        })
    }

    fn handle_result(&mut self, result: ParseResult, events: &mut Vec<ToolingEvent>) {
        if !self.enabled {
            return;
        }
        match result.parsed {
            Ok((parsed, fingerprint)) => {
                if self.fingerprints.get(&result.job.path) == Some(&fingerprint) {
                    return;
                }
                self.remember_fingerprint(result.job.path.clone(), fingerprint);
                if let Some(signal) = self.signal_for(result.job, parsed) {
                    events.push(ToolingEvent::Signal(signal));
                }
            }
            Err(failure) if failure.is_incomplete() && result.job.attempts < MAX_PARSE_RETRIES => {
                self.requeue(ParseJob {
                    attempts: result.job.attempts.saturating_add(1),
                    ..result.job
                });
            }
            Err(failure) => events.push(ToolingEvent::Status(WatcherStatus::ReportRejected {
                path: result.job.path,
                reason: failure.reason().to_owned(),
            })),
        }
    }

    fn remember_fingerprint(&mut self, path: PathBuf, fingerprint: FileFingerprint) {
        if !self.fingerprints.contains_key(&path) && self.fingerprints.len() >= MAX_FINGERPRINTS {
            if let Some(oldest_path) = self.fingerprints.keys().next().cloned() {
                let _ = self.fingerprints.remove(&oldest_path);
            }
        }
        let _ = self.fingerprints.insert(path, fingerprint);
    }

    fn requeue(&mut self, job: ParseJob) {
        if self.pending.len() >= MAX_PENDING_REPORTS {
            return;
        }
        let pending = PendingReport {
            repo_root: job.repo_root,
            repo_name: job.repo_name,
            path: job.path.clone(),
            source: job.source,
            framework: job.framework,
            last_seen: Instant::now(),
            attempts: job.attempts,
        };
        let _ = self.pending.insert(job.path, pending);
    }

    fn signal_for(&mut self, job: ParseJob, parsed: ParsedReport) -> Option<DevSignal> {
        let run_id = match &parsed {
            ParsedReport::Test { run_id, .. } => run_id.clone(),
            ParsedReport::Build { run_id, .. } => Some(run_id.clone()),
        };
        if let Some(run_id) = &run_id {
            let key = (job.repo_root.clone(), run_id.clone());
            if self.recent_run_ids.contains(&key) {
                return None;
            }
            if self.recent_run_ids.len() >= MAX_RECENT_RUN_IDS {
                let _ = self.recent_run_ids.pop_front();
            }
            self.recent_run_ids.push_back(key);
        }

        match parsed {
            ParsedReport::Test { summary, run_id } => Some(DevSignal::TestCompleted {
                repo_name: job.repo_name,
                repo_path: job.repo_root,
                report_path: job.path,
                run_id,
                summary,
            }),
            ParsedReport::Build { summary, run_id } => Some(DevSignal::BuildCompleted {
                repo_name: job.repo_name,
                repo_path: job.repo_root,
                report_path: job.path,
                run_id,
                summary,
            }),
        }
    }
}

impl Drop for ToolingPipeline {
    fn drop(&mut self) {
        let _ = self.control_tx.try_send(ParserControl::Shutdown);
        if let Some(handle) = self.parser_handle.take() {
            let _ = handle.join();
        }
    }
}

fn parser_loop(
    job_rx: &Receiver<ParseJob>,
    result_tx: &Sender<ParseResult>,
    control_rx: &Receiver<ParserControl>,
) {
    loop {
        if control_rx.try_recv().is_ok() {
            return;
        }
        let mut select = Select::new();
        let control_index = select.recv(control_rx);
        let job_index = select.recv(job_rx);
        let selected = select.select();
        if selected.index() == control_index {
            return;
        }
        if selected.index() != job_index {
            continue;
        }
        let Ok(job) = selected.recv(job_rx) else {
            return;
        };
        let parsed = parse_job(&job);
        if result_tx.try_send(ParseResult { job, parsed }).is_err() {
            // Le worker principal est arrêté ou saturé ; ne jamais bloquer ici,
            // afin que `Drop` puisse toujours joindre ce thread.
        }
    }
}

fn parse_job(job: &ParseJob) -> Result<(ParsedReport, FileFingerprint), ParseFailure> {
    let canonical_repo = job.repo_root.canonicalize().map_err(|error| {
        ParseFailure::Incomplete(format!("racine du dépôt indisponible : {error}"))
    })?;
    let canonical_path = job
        .path
        .canonicalize()
        .map_err(|error| ParseFailure::Incomplete(format!("rapport indisponible : {error}")))?;
    if !canonical_path.starts_with(&canonical_repo) {
        return Err(ParseFailure::Rejected(String::from(
            "le rapport sort de la racine du dépôt",
        )));
    }
    let parsed = parse_report(&canonical_path, &job.source, job.framework)?;
    let fingerprint = fingerprint(&canonical_path)?;
    Ok((parsed, fingerprint))
}

fn fingerprint(path: &Path) -> Result<FileFingerprint, ParseFailure> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| ParseFailure::Incomplete(format!("métadonnées : {error}")))?;
    if metadata.len() > MAX_XML_FILE_BYTES {
        return Err(ParseFailure::Rejected(String::from(
            "rapport trop grand pour l'empreinte",
        )));
    }
    let mut file = std::fs::File::open(path)
        .map_err(|error| ParseFailure::Incomplete(format!("empreinte : {error}")))?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut buffer = vec![0_u8; 32 * 1024].into_boxed_slice();
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| ParseFailure::Incomplete(format!("empreinte : {error}")))?;
        if read == 0 {
            break;
        }
        buffer[..read].hash(&mut hasher);
    }
    Ok(FileFingerprint {
        len: metadata.len(),
        modified: metadata.modified().ok(),
        content_hash: hasher.finish(),
    })
}

fn candidate_matches(path: &Path, format: ToolingReportFormat) -> bool {
    let extension = path
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .map(str::to_ascii_lowercase);
    match format {
        ToolingReportFormat::Auto => matches!(extension.as_deref(), Some("xml" | "trx" | "json")),
        ToolingReportFormat::Junit => extension.as_deref() == Some("xml"),
        ToolingReportFormat::Trx => extension.as_deref() == Some("trx"),
        ToolingReportFormat::JestJson | ToolingReportFormat::GremlinJson => {
            extension.as_deref() == Some("json")
        }
    }
}

fn infer_framework(repo_root: &Path, path: &Path) -> ReportFramework {
    if path
        .components()
        .any(|part| part.as_os_str() == "TestResults")
    {
        return ReportFramework::Dotnet;
    }
    if path.components().any(|part| part.as_os_str() == "nextest")
        || repo_root.join("Cargo.toml").is_file()
    {
        ReportFramework::Rust
    } else if repo_root.join("package.json").is_file() {
        ReportFramework::JavaScript
    } else if repo_root.join("pyproject.toml").is_file() || repo_root.join("pytest.ini").is_file() {
        ReportFramework::Python
    } else if repo_root.join("go.mod").is_file() {
        ReportFramework::Go
    } else {
        ReportFramework::Generic
    }
}
