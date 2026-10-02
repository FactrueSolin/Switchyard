// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! The deployment TOML file: current source, versioning, history, and the
//! single validate-persist-swap apply path every console mutation goes through.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use switchyard_runner::Runner;
use switchyard_server::ServerState;

/// Replaced versions kept beside the deployment file.
const HISTORY_LIMIT: usize = 25;

pub struct ConfigStore {
    path: PathBuf,
    history_dir: PathBuf,
    state: Mutex<StoreState>,
}

struct StoreState {
    source: String,
    version: u64,
}

/// The version a successful apply settled on.
#[derive(Debug, Clone, Copy)]
pub struct ApplyReport {
    pub version: u64,
}

/// One entry in the on-disk history.
#[derive(Debug, Clone, serde::Serialize)]
pub struct HistoryEntry {
    pub version: u64,
    pub bytes: u64,
}

/// A change that cannot be applied.
#[derive(Debug)]
pub enum ApplyError {
    /// The new source failed `Runner::from_toml` validation.
    Invalid(String),
    /// The file or history directory could not be written.
    Io(io::Error),
}

impl std::fmt::Display for ApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) => write!(f, "invalid deployment config: {message}"),
            Self::Io(error) => write!(f, "deployment file error: {error}"),
        }
    }
}

impl std::error::Error for ApplyError {}

impl From<io::Error> for ApplyError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl ConfigStore {
    /// Opens the deployment file. Fails when it is missing or unreadable.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let source = fs::read_to_string(&path)?;
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("deployment");
        let history_dir = path
            .parent()
            .map(|parent| parent.join(format!(".{file_name}.history")))
            .unwrap_or_else(|| PathBuf::from(format!(".{file_name}.history")));
        Ok(Self {
            path,
            history_dir,
            state: Mutex::new(StoreState { source, version: 1 }),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The current source and version.
    pub fn current(&self) -> (String, u64) {
        let state = self.state.lock();
        (state.source.clone(), state.version)
    }

    /// Validates `new_source` and, when it differs from the current one,
    /// records the replaced version in the history, persists it, and swaps
    /// the running deployment.
    pub fn apply(&self, server: &ServerState, new_source: &str) -> Result<ApplyReport, ApplyError> {
        let runner = Runner::from_toml(new_source)
            .map_err(|error| ApplyError::Invalid(error.to_string()))?;
        let mut state = self.state.lock();
        if new_source == state.source {
            return Ok(ApplyReport {
                version: state.version,
            });
        }
        self.persist(&state, new_source)?;
        state.source = new_source.to_string();
        state.version += 1;
        let version = state.version;
        drop(state);
        server.swap_deployment(runner);
        tracing::info!(version, path = %self.path.display(), "deployment config applied");
        Ok(ApplyReport { version })
    }

    /// Rebuilds the running deployment from the current source without
    /// touching the file. Used after a secret changes so routes pick it up.
    pub fn reload(&self, server: &ServerState) -> Result<ApplyReport, ApplyError> {
        let (source, version) = self.current();
        let runner =
            Runner::from_toml(&source).map_err(|error| ApplyError::Invalid(error.to_string()))?;
        server.swap_deployment(runner);
        Ok(ApplyReport { version })
    }

    fn persist(&self, state: &StoreState, new_source: &str) -> Result<(), ApplyError> {
        fs::create_dir_all(&self.history_dir)?;
        let history_file = self.history_dir.join(format!("{:06}.toml", state.version));
        fs::write(&history_file, &state.source)?;
        atomic_write(&self.path, new_source)?;
        prune_history(&self.history_dir, HISTORY_LIMIT);
        Ok(())
    }

    /// History entries, newest first.
    pub fn list_history(&self) -> io::Result<Vec<HistoryEntry>> {
        let entries = match fs::read_dir(&self.history_dir) {
            Ok(entries) => entries.filter_map(|entry| entry.ok()).collect::<Vec<_>>(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut out = Vec::new();
        for entry in entries {
            let name = entry.file_name().to_string_lossy().to_string();
            let Ok(version) = name.split('.').next().unwrap_or("").parse::<u64>() else {
                continue;
            };
            let metadata = entry.metadata()?;
            out.push(HistoryEntry {
                version,
                bytes: metadata.len(),
            });
        }
        out.sort_by_key(|entry| std::cmp::Reverse(entry.version));
        Ok(out)
    }

    pub fn history_source(&self, version: u64) -> io::Result<String> {
        fs::read_to_string(self.history_file(version))
    }

    fn history_file(&self, version: u64) -> PathBuf {
        self.history_dir.join(format!("{version:06}.toml"))
    }
}

/// Write a temp file beside the target, then rename over it.
fn atomic_write(path: &Path, contents: &str) -> io::Result<()> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("deployment");
    let tmp = path.with_file_name(format!(".{file_name}.tmp"));
    fs::write(&tmp, contents)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

fn prune_history(dir: &Path, limit: usize) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut versions: Vec<u64> = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            name.split('.').next()?.parse::<u64>().ok()
        })
        .collect();
    if versions.len() <= limit {
        return;
    }
    versions.sort_unstable();
    let keep = versions.len() - limit;
    for &version in &versions[..keep] {
        let _ = fs::remove_file(dir.join(format!("{version:06}.toml")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = r#"
schema_version = 1

[llm_clients.primary]
format = "openai_chat"
base_url = "https://example.test/v1"

[targets.weak]
id = "weak/model"
llm_client = "primary"

[routes.noop]
id = "switchyard/noop"
type = "noop"
"#;

    fn change(route_id: &str) -> String {
        SOURCE.replace("id = \"switchyard/noop\"", &format!("id = \"{route_id}\""))
    }

    #[test]
    fn apply_validates_persists_and_records_history() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deployment.toml");
        fs::write(&path, SOURCE).unwrap();
        let store = ConfigStore::open(&path).unwrap();
        assert_eq!(store.current().1, 1);

        let error = store.apply(&unused_server(), "not toml at all");
        assert!(matches!(error, Err(ApplyError::Invalid(_))));
        assert_eq!(fs::read_to_string(&path).unwrap(), SOURCE);

        store
            .apply(&unused_server(), &change("switchyard/other"))
            .unwrap();
        assert_eq!(store.current().1, 2);
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .contains("switchyard/other")
        );

        let history = store.list_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].version, 1);
        assert_eq!(store.history_source(1).unwrap(), SOURCE);
    }

    #[test]
    fn an_unchanged_source_is_a_noop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deployment.toml");
        fs::write(&path, SOURCE).unwrap();
        let store = ConfigStore::open(&path).unwrap();
        let report = store.apply(&unused_server(), SOURCE).unwrap();
        assert_eq!(report.version, 1);
        assert!(store.list_history().unwrap().is_empty());
    }

    // Apply and reload are the only store methods that take server state.
    // The tests only exercise the file side; swap_deployment on this state is
    // harmless because the source always validates.
    fn unused_server() -> ServerState {
        ServerState::from_runner(Runner::from_toml(SOURCE).unwrap()).unwrap()
    }
}
