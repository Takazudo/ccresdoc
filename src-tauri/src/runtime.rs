//! Runtime policy for the generated-docs source and app-owned local server.
//!
//! This module is deliberately independent of Tauri.  It owns the small,
//! security-sensitive decisions (port allocation, origin checks, readiness,
//! supersession and authored-vs-active state) while `main` supplies the actual
//! generator, watcher and child process.

use crate::settings::{
    ApplyImpact, ContentRevision, EffectiveSettings, SaveError, SettingsDraft, SettingsSnapshot,
    SettingsStore,
};
use html5ever::tendril::StrTendril;
use html5ever::tokenizer::{
    BufferQueue, Tag, TagKind, TagToken, Token, TokenSink, TokenSinkResult, Tokenizer,
    TokenizerOpts,
};
use html5ever::{local_name, ns};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;

pub const DOCS_PATH: &str = "/docs/";
pub const SHELL_MARKER: &str = "CCResDoc";
pub const CLAUDE_PATH: &str = "/docs/claude/";
pub const CODEX_PATH: &str = "/docs/codex/";
pub const MAX_BIND_ATTEMPTS: usize = 4;
/// A generated docs shell normally has one islands entry. Keep readiness
/// bounded even if a broken response contains a pathological number of
/// module scripts; an over-cap response remains stale until the next poll.
pub const MAX_MODULE_SCRIPT_PROBES: usize = 16;
const MAX_STATUS_LINE_BYTES: usize = 8 * 1024;

pub const CLAUDE_NAMESPACES: &[&str] = &[
    "claude",
    "claude-md",
    "claude-commands",
    "claude-skills",
    "claude-agents",
];
pub const CODEX_NAMESPACES: &[&str] = &[
    "codex",
    "codex-agents-md",
    "codex-config",
    "codex-agents",
    "codex-hooks",
    "codex-rules",
    "codex-skills",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceSelection {
    pub claude: bool,
    pub codex: bool,
}

impl ResourceSelection {
    pub fn from_effective(settings: &EffectiveSettings) -> Self {
        Self {
            claude: settings.claude_resources,
            codex: settings.codex_resources,
        }
    }

    pub fn detail_namespaces(self) -> impl Iterator<Item = &'static str> {
        CLAUDE_NAMESPACES[1..]
            .iter()
            .copied()
            .filter(move |_| self.claude)
            .chain(
                CODEX_NAMESPACES[1..]
                    .iter()
                    .copied()
                    .filter(move |_| self.codex),
            )
    }

    pub fn watcher_count(self) -> usize {
        usize::from(self.claude) + usize::from(self.codex)
    }
}

pub fn transition_marker(generation: u64) -> std::io::Result<String> {
    let mut random = [0_u8; 24];
    fill_random(&mut random)?;
    let random = random
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(format!("ccresdoc-generation-{generation}-{random}"))
}

#[cfg(unix)]
fn fill_random(bytes: &mut [u8]) -> std::io::Result<()> {
    std::fs::File::open("/dev/urandom")?.read_exact(bytes)
}

#[cfg(windows)]
fn fill_random(bytes: &mut [u8]) -> std::io::Result<()> {
    #[link(name = "bcrypt")]
    extern "system" {
        fn BCryptGenRandom(
            algorithm: *mut std::ffi::c_void,
            buffer: *mut u8,
            length: u32,
            flags: u32,
        ) -> i32;
    }
    const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 0x0000_0002;
    let length = u32::try_from(bytes.len())
        .map_err(|_| std::io::Error::other("random marker buffer is too large"))?;
    // SAFETY: a valid writable buffer and its exact length are passed to the
    // Windows system-preferred CSPRNG; a null algorithm handle is required by
    // BCRYPT_USE_SYSTEM_PREFERRED_RNG.
    let status = unsafe {
        BCryptGenRandom(
            std::ptr::null_mut(),
            bytes.as_mut_ptr(),
            length,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "BCryptGenRandom failed with NTSTATUS {status:#x}"
        )))
    }
}

pub fn overview_mdx(kind: &str, enabled: bool, categories: &[&str], marker: &str) -> String {
    let title = if kind == "claude" {
        "Claude Resources"
    } else {
        "Codex Resources"
    };
    let position = if kind == "claude" { 899 } else { 904 };
    let state = if enabled { "enabled" } else { "disabled" };
    let cards = if enabled && !categories.is_empty() {
        format!(
            "\n<CategoryNav categories={{{}}} />\n",
            serde_json::to_string(categories).expect("static category names serialize")
        )
    } else {
        String::new()
    };
    format!(
        "---\ntitle: {title}\ndescription: {title} are {state}.\nsidebar_position: {position}\n---\n\n<div data-ccresdoc-resource=\"{kind}\" data-ccresdoc-state=\"{state}\" data-ccresdoc-generation=\"{marker}\">\n\n{title} are **{state}** in CCResDoc settings.\n{cards}\n</div>\n"
    )
}

#[derive(Debug)]
pub struct ManagedTreeJournal {
    docs_dir: PathBuf,
    backup_dir: PathBuf,
    promoted: Vec<&'static str>,
    backed_up: Vec<&'static str>,
}

impl ManagedTreeJournal {
    pub fn promote(
        docs_dir: &Path,
        candidate_dir: &Path,
        backup_dir: &Path,
    ) -> std::io::Result<Self> {
        std::fs::create_dir_all(docs_dir)?;
        remove_exact(backup_dir)?;
        std::fs::create_dir_all(backup_dir)?;
        let mut journal = Self {
            docs_dir: docs_dir.to_path_buf(),
            backup_dir: backup_dir.to_path_buf(),
            promoted: Vec::new(),
            backed_up: Vec::new(),
        };
        let result = (|| {
            for name in CLAUDE_NAMESPACES.iter().chain(CODEX_NAMESPACES) {
                let live = docs_dir.join(name);
                if live.exists() {
                    std::fs::rename(&live, backup_dir.join(name))?;
                    journal.backed_up.push(name);
                }
                let candidate = candidate_dir.join(name);
                if candidate.exists() {
                    std::fs::rename(&candidate, &live)?;
                    journal.promoted.push(name);
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            let rollback = journal.rollback();
            return Err(rollback.err().unwrap_or(error));
        }
        Ok(journal)
    }

    pub fn rollback(mut self) -> std::io::Result<()> {
        let mut first_error = None;
        for name in self.promoted.drain(..).rev() {
            if let Err(error) = remove_exact(&self.docs_dir.join(name)) {
                first_error.get_or_insert(error);
            }
        }
        for name in self.backed_up.drain(..).rev() {
            if let Err(error) =
                std::fs::rename(self.backup_dir.join(name), self.docs_dir.join(name))
            {
                first_error.get_or_insert(error);
            }
        }
        if first_error.is_none() {
            if let Err(error) = remove_exact(&self.backup_dir) {
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    pub fn commit(self) -> std::io::Result<()> {
        remove_exact(&self.backup_dir)
    }
}

pub fn remove_exact(path: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            std::fs::remove_dir_all(path)
        }
        Ok(_) => std::fs::remove_file(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimePhase {
    Idle,
    Starting,
    Ready,
    SavedNotActive,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeSnapshot {
    pub authored: SettingsSnapshot,
    pub active: Option<EffectiveSettings>,
    pub phase: RuntimePhase,
    pub fallback_used: bool,
    pub generation: u64,
    pub diagnostic: Option<RuntimeDiagnostic>,
    pub generated: Option<ResourceCounts>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceCounts {
    pub claude_md: usize,
    pub claude_commands: usize,
    pub claude_skills: usize,
    pub claude_agents: usize,
    pub codex_agents_md: usize,
    pub codex_config: usize,
    pub codex_agents: usize,
    pub codex_hooks: usize,
    pub codex_rules: usize,
    pub codex_skills: usize,
    pub codex_warnings: usize,
}

impl RuntimeSnapshot {
    pub fn new(authored: SettingsSnapshot) -> Self {
        Self {
            authored,
            active: None,
            phase: RuntimePhase::Idle,
            fallback_used: false,
            generation: 0,
            diagnostic: None,
            generated: None,
        }
    }

    pub fn effective_port(&self) -> Option<u16> {
        self.active.as_ref().map(|settings| settings.effective_port)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeDiagnosticKind {
    WorkspaceUnavailable,
    ZfbBinaryMissing,
    SourceUnavailable,
    PreferredPortOccupied,
    BindRetryExhausted,
    GenerateFailed,
    PromotionFailed,
    WatchFailed,
    SpawnFailed,
    SidecarExited,
    Timeout,
    Superseded,
    RestoreFailed,
    RelaunchFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeDiagnostic {
    pub kind: RuntimeDiagnosticKind,
    pub preferred_port: u16,
    pub attempted_port: Option<u16>,
    pub message: String,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PortError {
    #[error("preferred loopback port {port} is occupied")]
    PreferredOccupied { port: u16 },
    #[error("could not allocate a loopback fallback port: {message}")]
    Allocation { message: String },
    #[error("the server lost every loopback bind race after {attempts} attempts")]
    RetryExhausted { attempts: usize },
}

pub trait PortBoundary {
    fn is_available(&mut self, port: u16) -> std::io::Result<bool>;
    fn fallback_candidate(&mut self) -> std::io::Result<u16>;
}

pub struct SystemPortBoundary;

impl PortBoundary for SystemPortBoundary {
    fn is_available(&mut self, port: u16) -> std::io::Result<bool> {
        match TcpListener::bind((Ipv4Addr::LOCALHOST, port)) {
            Ok(listener) => {
                drop(listener);
                Ok(true)
            }
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn fallback_candidate(&mut self) -> std::io::Result<u16> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = listener.local_addr()?.port();
        drop(listener);
        Ok(port)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortChoice {
    pub preferred_port: u16,
    pub effective_port: u16,
    pub fallback_used: bool,
}

pub fn choose_port(
    boundary: &mut impl PortBoundary,
    preferred_port: u16,
    fallback: bool,
) -> Result<PortChoice, PortError> {
    if boundary
        .is_available(preferred_port)
        .map_err(|error| PortError::Allocation {
            message: error.to_string(),
        })?
    {
        return Ok(PortChoice {
            preferred_port,
            effective_port: preferred_port,
            fallback_used: false,
        });
    }
    if !fallback {
        return Err(PortError::PreferredOccupied {
            port: preferred_port,
        });
    }
    let effective_port = boundary
        .fallback_candidate()
        .map_err(|error| PortError::Allocation {
            message: error.to_string(),
        })?;
    Ok(PortChoice {
        preferred_port,
        effective_port,
        fallback_used: true,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnAttempt<T> {
    Started(T),
    BindLost,
}

/// Select a preferred/fallback port and retry only explicit preflight-to-spawn
/// bind losses. Other spawn failures must be returned by the caller directly.
pub fn choose_and_spawn<T>(
    boundary: &mut impl PortBoundary,
    preferred_port: u16,
    fallback: bool,
    max_attempts: usize,
    mut spawn: impl FnMut(PortChoice) -> Result<SpawnAttempt<T>, String>,
) -> Result<(PortChoice, T), PortError> {
    let attempts = max_attempts.max(1);
    for attempt in 0..attempts {
        let choice = if attempt == 0 {
            choose_port(boundary, preferred_port, fallback)?
        } else {
            if !fallback {
                return Err(PortError::PreferredOccupied {
                    port: preferred_port,
                });
            }
            PortChoice {
                preferred_port,
                effective_port: boundary.fallback_candidate().map_err(|error| {
                    PortError::Allocation {
                        message: error.to_string(),
                    }
                })?,
                fallback_used: true,
            }
        };
        match spawn(choice).map_err(|message| PortError::Allocation { message })? {
            SpawnAttempt::Started(value) => return Ok((choice, value)),
            SpawnAttempt::BindLost => continue,
        }
    }
    Err(PortError::RetryExhausted { attempts })
}

pub fn docs_url(port: u16) -> String {
    format!("http://localhost:{port}{DOCS_PATH}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationDecision {
    Allow,
    OpenExternal,
    Reject,
}

pub fn navigation_decision(url: &url::Url, effective_port: Option<u16>) -> NavigationDecision {
    match url.scheme() {
        "tauri" | "asset" => NavigationDecision::Allow,
        "about" if url.as_str() == "about:blank" => NavigationDecision::Allow,
        "http" => {
            let loopback = match url.host() {
                Some(url::Host::Domain("localhost")) => true,
                Some(url::Host::Ipv4(ip)) => ip == Ipv4Addr::LOCALHOST,
                _ => false,
            };
            if loopback && url.port_or_known_default() == effective_port {
                NavigationDecision::Allow
            } else {
                NavigationDecision::OpenExternal
            }
        }
        "https" => NavigationDecision::OpenExternal,
        _ => NavigationDecision::Reject,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadinessState {
    HttpUnavailable,
    StaleContent,
    Ready,
}

pub fn classify_readiness(status: u16, body: &str, marker: &str) -> ReadinessState {
    if status != 200 {
        ReadinessState::HttpUnavailable
    } else if !body.contains(marker) {
        ReadinessState::StaleContent
    } else {
        ReadinessState::Ready
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ModuleScriptScan {
    paths: Vec<String>,
    has_island_markers: bool,
    over_probe_cap: bool,
}

struct ModuleScriptSink {
    scan: RefCell<ModuleScriptScan>,
    port: u16,
}

fn html_attribute<'a>(tag: &'a Tag, wanted: &str) -> Option<&'a str> {
    tag.attrs
        .iter()
        .find(|attribute| {
            attribute.name.ns == *ns!()
                && attribute
                    .name
                    .local
                    .to_string()
                    .eq_ignore_ascii_case(wanted)
        })
        .map(|attribute| attribute.value.as_ref())
}

impl TokenSink for ModuleScriptSink {
    type Handle = ();

    fn process_token(&self, token: Token, _line_number: u64) -> TokenSinkResult<()> {
        let TagToken(tag) = token else {
            return TokenSinkResult::Continue;
        };
        if tag.kind != TagKind::StartTag {
            return TokenSinkResult::Continue;
        }

        let mut scan = self.scan.borrow_mut();
        if tag.attrs.iter().any(|attribute| {
            attribute.name.ns == *ns!()
                && (attribute
                    .name
                    .local
                    .to_string()
                    .eq_ignore_ascii_case("data-zfb-island")
                    || attribute
                        .name
                        .local
                        .to_string()
                        .eq_ignore_ascii_case("data-zfb-island-skip-ssr"))
        }) {
            scan.has_island_markers = true;
        }

        if tag.name != local_name!("script") {
            return TokenSinkResult::Continue;
        }
        let is_module = html_attribute(&tag, "type").is_some_and(|value| {
            value
                .trim_matches(|character: char| character.is_ascii_whitespace())
                .eq_ignore_ascii_case("module")
        });
        let Some(src) = html_attribute(&tag, "src") else {
            return TokenSinkResult::Continue;
        };
        if !is_module {
            return TokenSinkResult::Continue;
        }
        let Some(path) = resolve_module_script_path(src, self.port) else {
            return TokenSinkResult::Continue;
        };
        if !scan.paths.contains(&path) {
            if scan.paths.len() >= MAX_MODULE_SCRIPT_PROBES {
                scan.over_probe_cap = true;
            } else {
                scan.paths.push(path);
            }
        }
        TokenSinkResult::Continue
    }
}

fn resolve_module_script_path(src: &str, port: u16) -> Option<String> {
    if src.is_empty() || src.contains(['\r', '\n']) {
        return None;
    }
    let src = src.trim_matches(|character: char| character.is_ascii_whitespace());
    if src.is_empty() {
        return None;
    }
    let base = url::Url::parse(&format!("http://localhost:{port}{DOCS_PATH}")).ok()?;
    let url = base.join(src).ok()?;
    if url.origin() != base.origin() || !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    let path = url.path();
    if path.is_empty() || path.contains(['\r', '\n']) {
        return None;
    }
    let mut request_path = path.to_owned();
    if let Some(query) = url.query() {
        request_path.push('?');
        request_path.push_str(query);
    }
    Some(request_path)
}

fn scan_module_scripts(body: &str, port: u16) -> ModuleScriptScan {
    let sink = ModuleScriptSink {
        scan: RefCell::new(ModuleScriptScan::default()),
        port,
    };
    let tokenizer = Tokenizer::new(sink, TokenizerOpts::default());
    let input = BufferQueue::default();
    input.push_back(StrTendril::from_slice(body));
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    tokenizer.sink.scan.into_inner()
}

/// Minimal in-process HTTP/1.0 probe.  The server is mandatory-loopback and
/// the response is bounded, so this avoids a shell/curl dependency without
/// introducing a general-purpose network client into the app.
pub fn probe_path(port: u16, path: &str, io_timeout: Duration) -> std::io::Result<(u16, String)> {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let mut stream = TcpStream::connect_timeout(&addr, io_timeout)?;
    stream.set_read_timeout(Some(io_timeout))?;
    stream.set_write_timeout(Some(io_timeout))?;
    stream.write_all(
        format!("GET {path} HTTP/1.0\r\nHost: localhost:{port}\r\nConnection: close\r\n\r\n")
            .as_bytes(),
    )?;
    let mut bytes = Vec::new();
    stream.take(2 * 1024 * 1024).read_to_end(&mut bytes)?;
    let response = String::from_utf8_lossy(&bytes);
    let (head, body) = response.split_once("\r\n\r\n").unwrap_or((&response, ""));
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    Ok((status, body.to_owned()))
}

/// Probe only the HTTP status line for a same-origin module entry. Dropping
/// the stream immediately after the line avoids reading the entry's often
/// hundreds-of-kilobytes JavaScript body on every readiness poll.
pub fn probe_status(port: u16, path: &str, io_timeout: Duration) -> std::io::Result<u16> {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let mut stream = TcpStream::connect_timeout(&addr, io_timeout)?;
    stream.set_read_timeout(Some(io_timeout))?;
    stream.set_write_timeout(Some(io_timeout))?;
    stream.write_all(
        format!("GET {path} HTTP/1.0\r\nHost: localhost:{port}\r\nConnection: close\r\n\r\n")
            .as_bytes(),
    )?;

    let mut line = Vec::new();
    let mut byte = [0_u8; 1];
    let deadline = Instant::now()
        .checked_add(io_timeout)
        .unwrap_or_else(Instant::now);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "timed out reading HTTP status line",
            ));
        }
        stream.set_read_timeout(Some(remaining))?;
        stream.read_exact(&mut byte)?;
        if byte[0] == b'\n' {
            break;
        }
        if line.len() >= MAX_STATUS_LINE_BYTES {
            return Ok(0);
        }
        line.push(byte[0]);
    }
    Ok(String::from_utf8_lossy(&line)
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(0))
}

pub fn probe_resource_readiness(
    port: u16,
    selection: ResourceSelection,
    marker: &str,
    io_timeout: Duration,
) -> ReadinessState {
    let Ok((shell_status, shell)) = probe_path(port, DOCS_PATH, io_timeout) else {
        return ReadinessState::HttpUnavailable;
    };
    if shell_status != 200 || !shell.contains(SHELL_MARKER) {
        return if shell_status == 200 {
            ReadinessState::StaleContent
        } else {
            ReadinessState::HttpUnavailable
        };
    }

    let scripts = scan_module_scripts(&shell, port);
    if scripts.over_probe_cap {
        return ReadinessState::StaleContent;
    }
    if scripts.has_island_markers && scripts.paths.is_empty() {
        return ReadinessState::StaleContent;
    }
    for path in scripts.paths {
        let status = match probe_status(port, &path, io_timeout) {
            Ok(status) => status,
            Err(_) => return ReadinessState::HttpUnavailable,
        };
        if status != 200 {
            return ReadinessState::StaleContent;
        }
    }
    for (path, kind, enabled) in [
        (CLAUDE_PATH, "claude", selection.claude),
        (CODEX_PATH, "codex", selection.codex),
    ] {
        let Ok((status, body)) = probe_path(port, path, io_timeout) else {
            return ReadinessState::HttpUnavailable;
        };
        let state = if enabled { "enabled" } else { "disabled" };
        if status != 200
            || !body.contains(&format!("data-ccresdoc-resource=\"{kind}\""))
            || !body.contains(&format!("data-ccresdoc-state=\"{state}\""))
            || !body.contains(marker)
        {
            return if status == 200 {
                ReadinessState::StaleContent
            } else {
                ReadinessState::HttpUnavailable
            };
        }
    }
    ReadinessState::Ready
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadyResult {
    Ready,
    Timeout,
    Superseded,
    SidecarExited { code: Option<i32> },
}

pub fn wait_for_ready(
    port: u16,
    timeout: Duration,
    generation: (&AtomicU64, u64),
    mut child_status: impl FnMut() -> Option<Option<i32>>,
    mut probe: impl FnMut(u16) -> ReadinessState,
) -> ReadyResult {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if generation.0.load(Ordering::SeqCst) != generation.1 {
            return ReadyResult::Superseded;
        }
        if let Some(code) = child_status() {
            return ReadyResult::SidecarExited { code };
        }
        if probe(port) == ReadinessState::Ready {
            return ReadyResult::Ready;
        }
        thread::sleep(Duration::from_millis(50));
    }
    ReadyResult::Timeout
}

/// Single apply/restart turnstile. The atomic lease serializes slow work; the
/// snapshot mutex is held only for cloning or publishing state, never while
/// persistence, generation, shutdown, spawn or readiness is running.
pub struct ApplyCoordinator {
    applying: AtomicBool,
    generation: AtomicU64,
    state: Mutex<RuntimeSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplyStatus {
    Active,
    SavedNotActive,
    SavedNoRestart,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeApplyResult {
    pub snapshot: RuntimeSnapshot,
    pub impact: ApplyImpact,
    pub status: ApplyStatus,
}

impl ApplyCoordinator {
    pub fn new(snapshot: SettingsSnapshot) -> Self {
        Self {
            applying: AtomicBool::new(false),
            generation: AtomicU64::new(0),
            state: Mutex::new(RuntimeSnapshot::new(snapshot)),
        }
    }

    pub fn generation(&self) -> &AtomicU64 {
        &self.generation
    }

    pub fn claim_generation(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn snapshot(&self) -> RuntimeSnapshot {
        lock_unpoisoned(&self.state).clone()
    }

    pub fn publish_starting(&self, authored: SettingsSnapshot, generation: u64) {
        let mut state = lock_unpoisoned(&self.state);
        state.authored = authored;
        state.phase = RuntimePhase::Starting;
        state.generation = generation;
        state.diagnostic = None;
    }

    pub fn publish_ready(
        &self,
        mut effective: EffectiveSettings,
        choice: PortChoice,
        generation: u64,
    ) {
        effective.effective_port = choice.effective_port;
        let mut state = lock_unpoisoned(&self.state);
        if state.generation != generation {
            return;
        }
        state.active = Some(effective);
        state.phase = RuntimePhase::Ready;
        state.fallback_used = choice.fallback_used;
        state.diagnostic = None;
    }

    pub fn publish_failed(&self, diagnostic: RuntimeDiagnostic, generation: u64) {
        let mut state = lock_unpoisoned(&self.state);
        if state.generation != generation {
            return;
        }
        state.phase = RuntimePhase::SavedNotActive;
        state.diagnostic = Some(diagnostic);
    }

    pub fn publish_generated(&self, generated: ResourceCounts, generation: u64) {
        let mut state = lock_unpoisoned(&self.state);
        if state.generation == generation {
            state.generated = Some(generated);
        }
    }

    pub fn clear_active(&self, generation: u64) {
        let mut state = lock_unpoisoned(&self.state);
        if state.generation == generation {
            state.active = None;
            state.fallback_used = false;
            state.generated = None;
        }
    }

    pub fn publish_stopped(&self, generation: u64) {
        let mut state = lock_unpoisoned(&self.state);
        state.generation = generation;
        state.active = None;
        state.phase = RuntimePhase::Stopped;
        state.fallback_used = false;
        state.generated = None;
    }

    pub fn with_serialized_apply<T>(&self, operation: impl FnOnce() -> T) -> T {
        while self
            .applying
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            thread::yield_now();
        }
        struct Release<'a>(&'a AtomicBool);
        impl Drop for Release<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
            }
        }
        let _release = Release(&self.applying);
        operation()
    }

    /// Publish a freshly reloaded TOML appearance without restarting the docs
    /// server or changing its active source/port contract.
    pub fn publish_authoritative_appearance(&self, snapshot: SettingsSnapshot) {
        let mut state = lock_unpoisoned(&self.state);
        let mode = snapshot.effective.appearance_mode.clone();
        let pack = snapshot.effective.theme_pack.clone();
        state.authored = snapshot;
        if let Some(active) = state.active.as_mut() {
            active.appearance_mode = mode;
            active.theme_pack = pack;
        }
    }

    /// Validate and persist a draft through the versioned store, then run at
    /// most one runtime transition. Appearance-only saves publish authored
    /// state without invoking `restart`. A failed restart keeps the last
    /// active snapshot and reports explicit saved-vs-active divergence.
    pub fn apply_settings(
        &self,
        store: &SettingsStore,
        draft: &SettingsDraft,
        expected_revision: Option<&ContentRevision>,
        restart: impl FnOnce(u64, &EffectiveSettings) -> Result<PortChoice, RuntimeDiagnostic>,
    ) -> Result<RuntimeApplyResult, SaveError> {
        self.with_serialized_apply(|| {
            let saved = store.save(draft, expected_revision)?;
            if !impact_requires_restart(&saved.impact) {
                let mut state = lock_unpoisoned(&self.state);
                state.authored = saved.snapshot;
                let appearance_mode = state.authored.effective.appearance_mode.clone();
                let theme_pack = state.authored.effective.theme_pack.clone();
                if let Some(active) = state.active.as_mut() {
                    active.appearance_mode = appearance_mode;
                    active.theme_pack = theme_pack;
                }
                let status = ApplyStatus::SavedNoRestart;
                return Ok(RuntimeApplyResult {
                    snapshot: state.clone(),
                    impact: saved.impact,
                    status,
                });
            }

            let generation = self.claim_generation();
            let effective = saved.snapshot.effective.clone();
            self.publish_starting(saved.snapshot, generation);
            let status = match restart(generation, &effective) {
                Ok(choice) => {
                    self.publish_ready(effective, choice, generation);
                    ApplyStatus::Active
                }
                Err(diagnostic) => {
                    self.publish_failed(diagnostic, generation);
                    ApplyStatus::SavedNotActive
                }
            };
            Ok(RuntimeApplyResult {
                snapshot: self.snapshot(),
                impact: saved.impact,
                status,
            })
        })
    }
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn impact_requires_restart(impact: &ApplyImpact) -> bool {
    matches!(impact, ApplyImpact::RestartRuntime)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{ActiveState, AppearanceMode, LoadStatus, SettingsDraft};
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::sync::atomic::AtomicUsize;
    use std::sync::{Arc, Barrier};
    use tempfile::TempDir;

    struct FakePorts {
        available: bool,
        candidates: VecDeque<u16>,
    }
    impl PortBoundary for FakePorts {
        fn is_available(&mut self, _: u16) -> std::io::Result<bool> {
            Ok(self.available)
        }
        fn fallback_candidate(&mut self) -> std::io::Result<u16> {
            self.candidates
                .pop_front()
                .ok_or_else(|| std::io::Error::other("exhausted"))
        }
    }

    fn settings(source: &str, port: u16) -> SettingsSnapshot {
        let effective = EffectiveSettings {
            claude_resources: true,
            codex_resources: false,
            claude_dir: Some(PathBuf::from(source)),
            codex_dir: None,
            appearance_mode: AppearanceMode::System,
            theme_pack: "default".into(),
            preferred_port: port,
            effective_port: port,
            fallback_to_free_port: true,
        };
        SettingsSnapshot {
            config_path: PathBuf::from("/tmp/config.toml"),
            file_exists: false,
            status: LoadStatus::Missing,
            revision: None,
            raw_content: None,
            authored: SettingsDraft::defaults(),
            effective: effective.clone(),
            active: ActiveState {
                uses_authored_settings: true,
                source_is_authored: true,
                preferred_port: port,
                effective_port: port,
            },
            validation: vec![],
        }
    }

    #[test]
    fn preferred_free_and_preferred_busy_fallback() {
        let mut free = FakePorts {
            available: true,
            candidates: vec![].into(),
        };
        assert_eq!(
            choose_port(&mut free, 4892, true).unwrap().effective_port,
            4892
        );
        let mut busy = FakePorts {
            available: false,
            candidates: vec![53001].into(),
        };
        let choice = choose_port(&mut busy, 4892, true).unwrap();
        assert_eq!(choice.effective_port, 53001);
        assert!(choice.fallback_used);
    }

    #[test]
    fn fallback_disabled_is_structured_and_does_not_touch_listener() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let error = choose_port(&mut SystemPortBoundary, port, false).unwrap_err();
        assert_eq!(error, PortError::PreferredOccupied { port });
        assert!(
            listener.local_addr().is_ok(),
            "unrelated listener must remain alive"
        );
        let client = TcpStream::connect((Ipv4Addr::LOCALHOST, port));
        assert!(
            client.is_ok(),
            "unrelated listener must still accept connections"
        );
    }

    #[test]
    fn occupied_foreign_listener_survives_real_fallback_selection() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let preferred = listener.local_addr().unwrap().port();
        let choice = choose_port(&mut SystemPortBoundary, preferred, true).unwrap();
        assert_eq!(choice.preferred_port, preferred);
        assert_ne!(choice.effective_port, preferred);
        assert!(choice.fallback_used);
        assert!(listener.local_addr().is_ok());
        assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, preferred)).is_ok());
        assert!(TcpListener::bind((Ipv4Addr::LOCALHOST, choice.effective_port)).is_ok());
    }

    #[test]
    fn retries_simulated_preflight_spawn_race_and_bounds_exhaustion() {
        let mut ports = FakePorts {
            available: true,
            candidates: vec![50001].into(),
        };
        let mut calls = 0;
        let (choice, value) = choose_and_spawn(&mut ports, 4892, true, 4, |_| {
            calls += 1;
            Ok(if calls == 1 {
                SpawnAttempt::BindLost
            } else {
                SpawnAttempt::Started("ok")
            })
        })
        .unwrap();
        assert_eq!(choice.effective_port, 50001);
        assert_eq!(value, "ok");

        let mut ports = FakePorts {
            available: false,
            candidates: vec![1, 2, 3].into(),
        };
        let error = choose_and_spawn(&mut ports, 4892, true, 3, |_| {
            Ok::<_, String>(SpawnAttempt::<()>::BindLost)
        })
        .unwrap_err();
        assert_eq!(error, PortError::RetryExhausted { attempts: 3 });
    }

    #[test]
    fn url_and_navigation_are_dynamic_and_structural() {
        assert_eq!(docs_url(53002), "http://localhost:53002/docs/");
        for raw in [
            "http://localhost:53002/docs/",
            "http://127.0.0.1:53002/docs/",
        ] {
            assert_eq!(
                navigation_decision(&url::Url::parse(raw).unwrap(), Some(53002)),
                NavigationDecision::Allow
            );
        }
        for raw in [
            "http://localhost:4892/docs/",
            "http://127.0.0.2:53002/",
            "http://[::1]:53002/docs/",
            "http://192.168.1.2:53002/",
            "https://localhost:53002/",
        ] {
            assert_ne!(
                navigation_decision(&url::Url::parse(raw).unwrap(), Some(53002)),
                NavigationDecision::Allow
            );
        }
        assert_eq!(
            navigation_decision(
                &url::Url::parse("javascript:alert(1)").unwrap(),
                Some(53002)
            ),
            NavigationDecision::Reject
        );
        assert_eq!(
            navigation_decision(&url::Url::parse("about:blank").unwrap(), None),
            NavigationDecision::Allow
        );
        assert_eq!(
            navigation_decision(&url::Url::parse("about:srcdoc").unwrap(), None),
            NavigationDecision::Reject
        );
    }

    #[test]
    fn readiness_rejects_stale_content_and_handles_terminal_states() {
        assert_eq!(
            classify_readiness(200, "old shell", "current"),
            ReadinessState::StaleContent
        );
        assert_eq!(
            classify_readiness(503, "current", "current"),
            ReadinessState::HttpUnavailable
        );
        let generation = AtomicU64::new(1);
        assert_eq!(
            wait_for_ready(
                1,
                Duration::from_millis(20),
                (&generation, 1),
                || None,
                |_| ReadinessState::HttpUnavailable
            ),
            ReadyResult::Timeout
        );
        assert_eq!(
            wait_for_ready(
                1,
                Duration::from_secs(1),
                (&generation, 1),
                || Some(Some(7)),
                |_| ReadinessState::Ready
            ),
            ReadyResult::SidecarExited { code: Some(7) }
        );
        generation.store(2, Ordering::SeqCst);
        assert_eq!(
            wait_for_ready(
                1,
                Duration::from_secs(1),
                (&generation, 1),
                || None,
                |_| ReadinessState::Ready
            ),
            ReadyResult::Superseded
        );
    }

    #[test]
    fn module_script_parser_accepts_all_attribute_quoting_and_orders() {
        let scan = scan_module_scripts(
            r#"
                <script src=/assets/unquoted.js type=module></script>
                <script type='module' src='/assets/single.js'></script>
                <script type="module" src="assets/double.js"></script>
                <script src=/assets/not-module.js type=text/javascript></script>
            "#,
            4892,
        );
        assert_eq!(
            scan.paths,
            vec![
                "/assets/unquoted.js",
                "/assets/single.js",
                "/docs/assets/double.js",
            ]
        );
        assert!(!scan.has_island_markers);
        assert!(!scan.over_probe_cap);
    }

    #[test]
    fn module_script_parser_ignores_cross_origin_and_rejects_crlf_sources() {
        let html = "<script type=module src=https://cdn.example.test/islands.js></script>\n"
            .to_owned()
            + "<script type=module src=\"/assets/bad\r\n.js\"></script>";
        let scan = scan_module_scripts(&html, 4892);
        assert!(scan.paths.is_empty());
    }

    #[test]
    fn module_script_parser_deduplicates_paths_and_tracks_island_markers() {
        let scan = scan_module_scripts(
            r#"
                <div data-zfb-island="true">
                  <script type=module src=/assets/islands.js></script>
                  <script src=/assets/islands.js type=module></script>
                </div>
            "#,
            4892,
        );
        assert_eq!(scan.paths, vec!["/assets/islands.js"]);
        assert!(scan.has_island_markers);
    }

    #[test]
    fn semantic_readiness_probes_every_module_entry_and_deduplicates_requests() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let script_requests = Arc::new(AtomicUsize::new(0));
        let script_requests_for_server = script_requests.clone();
        let server = thread::spawn(move || {
            for _ in 0..4 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0_u8; 4096];
                let count = stream.read(&mut request).unwrap();
                let request = String::from_utf8_lossy(&request[..count]);
                let path = request.split_whitespace().nth(1).unwrap_or_default();
                if path == "/assets/islands.js" {
                    script_requests_for_server.fetch_add(1, Ordering::SeqCst);
                    write!(stream, "HTTP/1.0 200 OK\r\nContent-Length: 0\r\n\r\n").unwrap();
                    continue;
                }
                let body = match path {
                    DOCS_PATH => format!(
                        "{SHELL_MARKER}<div data-zfb-island><script src=/assets/islands.js type=module><script type='module' src=\"/assets/islands.js\">"
                    ),
                    CLAUDE_PATH => {
                        "data-ccresdoc-resource=\"claude\" data-ccresdoc-state=\"disabled\" generation-current"
                            .to_owned()
                    }
                    CODEX_PATH => {
                        "data-ccresdoc-resource=\"codex\" data-ccresdoc-state=\"disabled\" generation-current"
                            .to_owned()
                    }
                    _ => String::new(),
                };
                write!(
                    stream,
                    "HTTP/1.0 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
        });
        assert_eq!(
            probe_resource_readiness(
                port,
                ResourceSelection {
                    claude: false,
                    codex: false,
                },
                "generation-current",
                Duration::from_secs(1),
            ),
            ReadinessState::Ready
        );
        server.join().unwrap();
        assert_eq!(script_requests.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn semantic_readiness_maps_module_status_failure_to_stale_content() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0_u8; 4096];
                let count = stream.read(&mut request).unwrap();
                let request = String::from_utf8_lossy(&request[..count]);
                let path = request.split_whitespace().nth(1).unwrap_or_default();
                if path == "/assets/islands.js" {
                    write!(
                        stream,
                        "HTTP/1.0 404 Not Found\r\nContent-Length: 0\r\n\r\n"
                    )
                    .unwrap();
                } else {
                    let body = format!(
                        "{SHELL_MARKER}<div data-zfb-island><script type=module src=/assets/islands.js>"
                    );
                    write!(
                        stream,
                        "HTTP/1.0 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                        body.len(),
                        body
                    )
                    .unwrap();
                }
            }
        });
        assert_eq!(
            probe_resource_readiness(
                port,
                ResourceSelection {
                    claude: false,
                    codex: false,
                },
                "generation-current",
                Duration::from_secs(1),
            ),
            ReadinessState::StaleContent
        );
        server.join().unwrap();
    }

    #[test]
    fn semantic_readiness_maps_module_connection_failure_to_http_unavailable() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).unwrap();
            let body = format!(
                "{SHELL_MARKER}<div data-zfb-island><script type=module src=/assets/islands.js>"
            );
            write!(
                stream,
                "HTTP/1.0 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });
        let state = probe_resource_readiness(
            port,
            ResourceSelection {
                claude: false,
                codex: false,
            },
            "generation-current",
            Duration::from_secs(1),
        );
        server.join().unwrap();
        assert_eq!(state, ReadinessState::HttpUnavailable);
    }

    #[test]
    fn semantic_readiness_rejects_island_markers_without_a_local_module_entry() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).unwrap();
            let body = format!("{SHELL_MARKER}<div data-zfb-island></div>");
            write!(
                stream,
                "HTTP/1.0 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });
        assert_eq!(
            probe_resource_readiness(
                port,
                ResourceSelection {
                    claude: false,
                    codex: false,
                },
                "generation-current",
                Duration::from_secs(1),
            ),
            ReadinessState::StaleContent
        );
        server.join().unwrap();
    }

    #[test]
    fn semantic_readiness_ignores_cross_origin_module_entries() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            for _ in 0..3 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0_u8; 4096];
                let count = stream.read(&mut request).unwrap();
                let request = String::from_utf8_lossy(&request[..count]);
                let path = request.split_whitespace().nth(1).unwrap_or_default();
                let body = match path {
                    DOCS_PATH => format!(
                        "{SHELL_MARKER}<script type=module src=https://cdn.example.test/islands.js>"
                    ),
                    CLAUDE_PATH => {
                        "data-ccresdoc-resource=\"claude\" data-ccresdoc-state=\"disabled\" generation-current"
                            .to_owned()
                    }
                    CODEX_PATH => {
                        "data-ccresdoc-resource=\"codex\" data-ccresdoc-state=\"disabled\" generation-current"
                            .to_owned()
                    }
                    _ => String::new(),
                };
                write!(
                    stream,
                    "HTTP/1.0 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
        });
        assert_eq!(
            probe_resource_readiness(
                port,
                ResourceSelection {
                    claude: false,
                    codex: false,
                },
                "generation-current",
                Duration::from_secs(1),
            ),
            ReadinessState::Ready
        );
        server.join().unwrap();
    }

    #[test]
    fn four_state_selection_prunes_every_disabled_detail_namespace() {
        for (claude, codex) in [(false, false), (true, false), (false, true), (true, true)] {
            let selection = ResourceSelection { claude, codex };
            let names = selection.detail_namespaces().collect::<Vec<_>>();
            assert_eq!(names.iter().any(|name| name.starts_with("claude-")), claude);
            assert_eq!(names.iter().any(|name| name.starts_with("codex-")), codex);
            assert!(!names.contains(&"claude"));
            assert!(!names.contains(&"codex"));
            assert_eq!(
                selection.watcher_count(),
                usize::from(claude) + usize::from(codex)
            );
        }
    }

    #[test]
    fn transition_markers_are_generation_scoped_and_unique() {
        let first = transition_marker(41).unwrap();
        let second = transition_marker(41).unwrap();
        let next = transition_marker(42).unwrap();
        assert!(first.starts_with("ccresdoc-generation-41-"));
        assert!(next.starts_with("ccresdoc-generation-42-"));
        assert_ne!(first, second);
        assert_ne!(first, next);
    }

    #[test]
    fn coordinator_overview_has_selection_and_generation_markers() {
        let enabled = overview_mdx("claude", true, &["claude-skills"], "generation-x");
        assert!(enabled.contains("data-ccresdoc-state=\"enabled\""));
        assert!(enabled.contains("data-ccresdoc-generation=\"generation-x\""));
        assert!(enabled.contains("claude-skills"));
        let disabled = overview_mdx("codex", false, &["codex-skills"], "generation-y");
        assert!(disabled.contains("data-ccresdoc-state=\"disabled\""));
        assert!(!disabled.contains("<CategoryNav"));
    }

    #[test]
    fn managed_tree_journal_rolls_back_only_exact_owned_namespaces() {
        let temp = TempDir::new().unwrap();
        let docs = temp.path().join("docs");
        let candidate = temp.path().join("candidate");
        let backup = temp.path().join("backup");
        std::fs::create_dir_all(docs.join("claude-skills")).unwrap();
        std::fs::write(docs.join("claude-skills/old.mdx"), "old").unwrap();
        std::fs::create_dir_all(docs.join("foreign")).unwrap();
        std::fs::write(docs.join("foreign/keep"), "foreign").unwrap();
        std::fs::create_dir_all(candidate.join("claude-skills")).unwrap();
        std::fs::write(candidate.join("claude-skills/new.mdx"), "new").unwrap();
        std::fs::create_dir_all(candidate.join("claude")).unwrap();
        std::fs::write(candidate.join("claude/index.mdx"), "candidate").unwrap();
        std::fs::create_dir_all(candidate.join("codex")).unwrap();
        std::fs::write(candidate.join("codex/index.mdx"), "disabled").unwrap();

        let journal = ManagedTreeJournal::promote(&docs, &candidate, &backup).unwrap();
        assert!(docs.join("claude-skills/new.mdx").exists());
        assert!(docs.join("foreign/keep").exists());
        journal.rollback().unwrap();
        assert!(docs.join("claude-skills/old.mdx").exists());
        assert!(!docs.join("claude-skills/new.mdx").exists());
        assert!(docs.join("foreign/keep").exists());
        assert!(!backup.exists());
    }

    #[test]
    fn in_process_probe_targets_dynamic_loopback_docs_path() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let count = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..count]);
            assert!(request.starts_with("GET /docs/ HTTP/1.0\r\n"));
            let body = "<nav>current</nav>";
            write!(
                stream,
                "HTTP/1.0 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });
        let (status, body) = probe_path(port, DOCS_PATH, Duration::from_secs(1)).unwrap();
        server.join().unwrap();
        assert_eq!(
            classify_readiness(status, &body, "current"),
            ReadinessState::Ready
        );
    }

    #[test]
    fn semantic_readiness_accepts_all_four_selection_states() {
        for selection in [
            ResourceSelection {
                claude: false,
                codex: false,
            },
            ResourceSelection {
                claude: true,
                codex: false,
            },
            ResourceSelection {
                claude: false,
                codex: true,
            },
            ResourceSelection {
                claude: true,
                codex: true,
            },
        ] {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            let port = listener.local_addr().unwrap().port();
            let server = thread::spawn(move || {
                for _ in 0..3 {
                    let (mut stream, _) = listener.accept().unwrap();
                    let mut request = [0_u8; 1024];
                    let count = stream.read(&mut request).unwrap();
                    let request = String::from_utf8_lossy(&request[..count]);
                    let body = if request.starts_with("GET /docs/ HTTP") {
                        SHELL_MARKER.to_string()
                    } else if request.starts_with("GET /docs/claude/ HTTP") {
                        format!(
                            "data-ccresdoc-resource=\"claude\" data-ccresdoc-state=\"{}\" generation-current",
                            if selection.claude { "enabled" } else { "disabled" }
                        )
                    } else {
                        format!(
                            "data-ccresdoc-resource=\"codex\" data-ccresdoc-state=\"{}\" generation-current",
                            if selection.codex { "enabled" } else { "disabled" }
                        )
                    };
                    write!(
                        stream,
                        "HTTP/1.0 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                        body.len(),
                        body
                    )
                    .unwrap();
                }
            });
            assert_eq!(
                probe_resource_readiness(
                    port,
                    selection,
                    "generation-current",
                    Duration::from_secs(1)
                ),
                ReadinessState::Ready
            );
            server.join().unwrap();
        }
    }

    #[test]
    fn source_switch_and_failed_apply_preserve_previous_active_runtime() {
        let coordinator = ApplyCoordinator::new(settings("/old", 4892));
        let first = coordinator.claim_generation();
        coordinator.publish_starting(settings("/old", 4892), first);
        coordinator.publish_ready(
            settings("/old", 4892).effective,
            PortChoice {
                preferred_port: 4892,
                effective_port: 50000,
                fallback_used: true,
            },
            first,
        );
        let second = coordinator.claim_generation();
        coordinator.publish_starting(settings("/new", 6000), second);
        coordinator.publish_failed(
            RuntimeDiagnostic {
                kind: RuntimeDiagnosticKind::GenerateFailed,
                preferred_port: 6000,
                attempted_port: None,
                message: "bad source".into(),
            },
            second,
        );
        let snapshot = coordinator.snapshot();
        assert_eq!(
            snapshot.authored.effective.claude_dir,
            Some(PathBuf::from("/new"))
        );
        assert_eq!(
            snapshot.active.unwrap().claude_dir,
            Some(PathBuf::from("/old"))
        );
        assert_eq!(snapshot.phase, RuntimePhase::SavedNotActive);
    }

    #[test]
    fn appearance_has_zero_restart_and_runtime_diff_has_one() {
        assert!(!impact_requires_restart(&ApplyImpact::None));
        assert!(!impact_requires_restart(&ApplyImpact::AppearanceOnly));
        assert!(impact_requires_restart(&ApplyImpact::RestartRuntime));
    }

    #[test]
    fn external_appearance_reload_updates_display_without_source_or_port_restart() {
        let coordinator = ApplyCoordinator::new(settings("/old", 4892));
        let generation = coordinator.claim_generation();
        coordinator.publish_starting(settings("/old", 4892), generation);
        coordinator.publish_ready(
            settings("/old", 4892).effective,
            PortChoice {
                preferred_port: 4892,
                effective_port: 51000,
                fallback_used: true,
            },
            generation,
        );
        let mut edited = settings("/new-authored-but-not-active", 6000);
        edited.effective.appearance_mode = AppearanceMode::Dark;
        edited.effective.theme_pack = "paper".into();
        coordinator.publish_authoritative_appearance(edited);
        let current = coordinator.snapshot();
        let active = current.active.unwrap();
        assert_eq!(active.claude_dir, Some(PathBuf::from("/old")));
        assert_eq!(active.effective_port, 51000);
        assert_eq!(active.appearance_mode, AppearanceMode::Dark);
        assert_eq!(active.theme_pack, "paper");
    }

    #[test]
    fn serialized_apply_never_overlaps_and_generation_supersedes() {
        let coordinator = Arc::new(ApplyCoordinator::new(settings("/old", 4892)));
        let barrier = Arc::new(Barrier::new(3));
        let inside = Arc::new(AtomicU64::new(0));
        let overlaps = Arc::new(AtomicU64::new(0));
        let mut joins = Vec::new();
        for _ in 0..2 {
            let (c, b, i, o) = (
                coordinator.clone(),
                barrier.clone(),
                inside.clone(),
                overlaps.clone(),
            );
            joins.push(thread::spawn(move || {
                b.wait();
                c.with_serialized_apply(|| {
                    if i.fetch_add(1, Ordering::SeqCst) != 0 {
                        o.fetch_add(1, Ordering::SeqCst);
                    }
                    thread::sleep(Duration::from_millis(20));
                    i.fetch_sub(1, Ordering::SeqCst);
                });
            }));
        }
        barrier.wait();
        for join in joins {
            join.join().unwrap();
        }
        assert_eq!(overlaps.load(Ordering::SeqCst), 0);
        let old = coordinator.claim_generation();
        let new = coordinator.claim_generation();
        assert_ne!(old, new);
        assert_eq!(coordinator.generation().load(Ordering::SeqCst), new);
    }

    #[test]
    fn persisted_apply_restarts_once_but_appearance_never_restarts() {
        let temp = TempDir::new().unwrap();
        let source_a = temp.path().join("a");
        let source_b = temp.path().join("b");
        std::fs::create_dir_all(&source_a).unwrap();
        std::fs::create_dir_all(&source_b).unwrap();
        let store = SettingsStore::new(temp.path().join("config.toml"), temp.path().into());
        let initial = store.load();
        let coordinator = ApplyCoordinator::new(initial.clone());

        let mut appearance = initial.authored.clone();
        appearance.claude_dir = source_a.to_string_lossy().into_owned();
        appearance.appearance_mode = "dark".into();
        let restarts = AtomicU64::new(0);
        let first = coordinator
            .apply_settings(&store, &appearance, initial.revision.as_ref(), |_, _| {
                restarts.fetch_add(1, Ordering::SeqCst);
                Ok(PortChoice {
                    preferred_port: 4892,
                    effective_port: 4892,
                    fallback_used: false,
                })
            })
            .unwrap();
        // The source changed from the project default, so the first save is a
        // runtime transition.
        assert_eq!(first.status, ApplyStatus::Active);
        assert_eq!(restarts.load(Ordering::SeqCst), 1);

        let mut only_appearance = first.snapshot.authored.authored.clone();
        only_appearance.appearance_mode = "light".into();
        let second = coordinator
            .apply_settings(
                &store,
                &only_appearance,
                first.snapshot.authored.revision.as_ref(),
                |_, _| {
                    restarts.fetch_add(1, Ordering::SeqCst);
                    unreachable!("appearance-only save must not restart")
                },
            )
            .unwrap();
        assert_eq!(second.status, ApplyStatus::SavedNoRestart);
        assert_eq!(restarts.load(Ordering::SeqCst), 1);

        let mut source_switch = second.snapshot.authored.authored.clone();
        source_switch.claude_dir = source_b.to_string_lossy().into_owned();
        let third = coordinator
            .apply_settings(
                &store,
                &source_switch,
                second.snapshot.authored.revision.as_ref(),
                |_, effective| {
                    restarts.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(
                        effective.claude_dir,
                        Some(std::fs::canonicalize(&source_b).unwrap())
                    );
                    Err(RuntimeDiagnostic {
                        kind: RuntimeDiagnosticKind::GenerateFailed,
                        preferred_port: effective.preferred_port,
                        attempted_port: None,
                        message: "simulated".into(),
                    })
                },
            )
            .unwrap();
        assert_eq!(third.status, ApplyStatus::SavedNotActive);
        assert_eq!(restarts.load(Ordering::SeqCst), 2);
        assert_eq!(
            third.snapshot.active.unwrap().claude_dir,
            Some(std::fs::canonicalize(&source_a).unwrap())
        );
    }
}
