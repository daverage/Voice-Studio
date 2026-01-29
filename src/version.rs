use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use semver::Version;
use serde::Deserialize;
use serde_json;
use ureq;

use nih_plug_vizia::vizia::prelude::{ContextProxy, Data};

const GITHUB_RELEASE_ENDPOINT: &str =
    "https://api.github.com/repos/daverage/Voice-Studio/releases/latest";

static VERSION_CHECK_STARTED: AtomicBool = AtomicBool::new(false);

/// The UI state that describes the current version status.
#[derive(Clone, Data)]
pub struct VersionUiState {
    pub label: String,
    pub detail: String,
    pub status: VersionStatus,
    pub release_url: Option<String>,
    pub latest_version: Option<String>,
}

impl VersionUiState {
    pub fn checking() -> Self {
        let current = current_version();
        Self {
            label: format!("VxCleaner {}", current),
            detail: "Checking GitHub for the latest release".into(),
            status: VersionStatus::Checking,
            release_url: None,
            latest_version: None,
        }
    }

    pub fn up_to_date(release: &RemoteRelease) -> Self {
        let current = current_version();
        Self {
            label: format!("VxCleaner {} (up to date)", current),
            detail: format!("Latest release: {} ({})", release.version, release.tag),
            status: VersionStatus::UpToDate,
            release_url: None,
            latest_version: Some(release.version.to_string()),
        }
    }

    pub fn update_available(release: &RemoteRelease) -> Self {
        let current = current_version();
        Self {
            label: format!("VxCleaner {} (update available)", current),
            detail: format!("Newest release: {} ({})", release.version, release.tag),
            status: VersionStatus::UpdateAvailable,
            release_url: Some(release.url.clone()),
            latest_version: Some(release.version.to_string()),
        }
    }

    pub fn error(message: &str) -> Self {
        let current = current_version();
        Self {
            label: format!("VxCleaner {} (update check failed)", current),
            detail: message.to_string(),
            status: VersionStatus::Error,
            release_url: None,
            latest_version: None,
        }
    }
}

/// The particle status of the version check.
#[derive(Clone, Copy, Data, PartialEq, Eq)]
pub enum VersionStatus {
    Checking,
    UpToDate,
    UpdateAvailable,
    Error,
}

/// Remote release metadata returned by GitHub.
#[derive(Debug, Clone)]
pub struct RemoteRelease {
    pub version: Version,
    pub url: String,
    pub tag: String,
    pub published_at: Option<String>,
}

/// Events emitted when the version checker has new data.
#[derive(Clone)]
pub enum VersionEvent {
    Update(VersionUiState),
}

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn spawn_version_check(proxy: Arc<Mutex<Option<ContextProxy>>>) {
    if VERSION_CHECK_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }

    thread::spawn(move || match fetch_latest_release() {
        Ok(release) => {
            let current =
                Version::parse(current_version()).unwrap_or_else(|_| Version::new(0, 0, 0));
            if release.version > current {
                let info = VersionUiState::update_available(&release);
                let _ = crate::vs_log!(
                    "Version check: latest release {} is newer than current {}",
                    release.version,
                    current
                );
                notify_ui(proxy.clone(), info);
            } else {
                let info = VersionUiState::up_to_date(&release);
                notify_ui(proxy.clone(), info);
            }
        }
        Err(err) => {
            let info = VersionUiState::error(&err.to_string());
            notify_ui(proxy.clone(), info);
        }
    });
}

fn notify_ui(proxy: Arc<Mutex<Option<ContextProxy>>>, state: VersionUiState) {
    if let Ok(mut guard) = proxy.lock() {
        if let Some(context_proxy) = guard.as_mut() {
            let mut emitter = context_proxy.clone();
            let _ = emitter.emit(VersionEvent::Update(state));
        }
    }
}

fn fetch_latest_release() -> anyhow::Result<RemoteRelease> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(5)))
        .timeout_recv_response(Some(Duration::from_secs(5)))
        .timeout_recv_body(Some(Duration::from_secs(5)))
        .build()
        .into();
    let mut response = agent
        .get(GITHUB_RELEASE_ENDPOINT)
        .header("User-Agent", "VxCleaner Version Checker")
        .header("Accept", "application/vnd.github+json")
        .call()?;

    let body = response.body_mut().read_to_string()?;
    let release: GitHubRelease = serde_json::from_str(&body)?;
    let parsed_version = normalize_tag(&release.tag_name)?;
    Ok(RemoteRelease {
        version: parsed_version,
        url: release.html_url,
        tag: release.tag_name,
        published_at: release.published_at,
    })
}

fn normalize_tag(tag: &str) -> anyhow::Result<Version> {
    let trimmed = tag.trim();
    let cleaned = trimmed
        .trim_start_matches(|c: char| !c.is_ascii_digit())
        .trim_start_matches('v')
        .trim_start_matches('V');

    semver::Version::parse(cleaned)
        .map_err(|e| anyhow::anyhow!("Failed to parse tag {}: {}", tag, e))
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    published_at: Option<String>,
}
