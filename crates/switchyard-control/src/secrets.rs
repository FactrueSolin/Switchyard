// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Process secrets backing the `api_key_env` values the deployment references.
//!
//! Stored as a flat TOML file of `NAME = "value"` pairs, kept at 0600, and
//! exported into the process environment so the runner's env-var convention
//! works unchanged.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use toml_edit::{DocumentMut, value};

pub struct SecretsStore {
    path: PathBuf,
    secrets: Mutex<BTreeMap<String, String>>,
}

impl SecretsStore {
    /// Opens the secrets file, or starts an empty store when it does not exist yet.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let secrets = if path.exists() {
            let source = fs::read_to_string(&path)?;
            parse(&source)?
        } else {
            BTreeMap::new()
        };
        Ok(Self {
            path,
            secrets: Mutex::new(secrets),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Secret names, never values.
    pub fn list(&self) -> Vec<String> {
        self.secrets.lock().keys().cloned().collect()
    }

    /// Resolves a name against the store first, then the process environment.
    pub fn resolve(&self, name: &str) -> Option<String> {
        self.secrets
            .lock()
            .get(name)
            .cloned()
            .or_else(|| std::env::var(name).ok())
    }

    /// Stores, persists, and exports a secret.
    pub fn set(&self, name: &str, value: &str) -> io::Result<()> {
        let valid = !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !valid {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "secret name {name:?} must contain only letters, digits, dashes, and underscores"
                ),
            ));
        }
        self.secrets
            .lock()
            .insert(name.to_string(), value.to_string());
        self.persist()?;
        // Edition 2024 marks process-env mutation unsafe: single-threaded at
        // startup, and this process owns its environment afterwards.
        unsafe { std::env::set_var(name, value) };
        Ok(())
    }

    /// Removes a secret and its exported variable. Returns whether it existed.
    pub fn remove(&self, name: &str) -> bool {
        let removed = self.secrets.lock().remove(name).is_some();
        if removed {
            let _ = self.persist();
            unsafe { std::env::remove_var(name) };
        }
        removed
    }

    /// Exports every stored secret into the process environment.
    ///
    /// Runs before the first `Runner::load`, so a deployment file can
    /// reference keys the store provides.
    pub fn apply_to_env(&self) {
        for (name, value) in self.secrets.lock().iter() {
            unsafe { std::env::set_var(name, value) };
        }
    }

    fn persist(&self) -> io::Result<()> {
        let mut doc = DocumentMut::new();
        for (name, secret) in self.secrets.lock().iter() {
            doc.insert(name, value(secret.clone()));
        }
        fs::write(&self.path, doc.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&self.path, fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }
}

fn parse(source: &str) -> io::Result<BTreeMap<String, String>> {
    let doc: DocumentMut = source.parse().map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("secrets file is not TOML: {error}"),
        )
    })?;
    let mut out = BTreeMap::new();
    for (key, item) in doc.iter() {
        let value = item.as_str().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("secret {key} must be a string"),
            )
        })?;
        out.insert(key.to_string(), value.to_string());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.toml");
        let store = SecretsStore::open(&path).unwrap();
        store.set("PROBE_KEY", "abc").unwrap();
        assert_eq!(store.list(), vec!["PROBE_KEY".to_string()]);
        assert_eq!(store.resolve("PROBE_KEY"), Some("abc".to_string()));

        let reopened = SecretsStore::open(&path).unwrap();
        assert_eq!(reopened.resolve("PROBE_KEY"), Some("abc".to_string()));
        assert!(reopened.remove("PROBE_KEY"));
        assert_eq!(reopened.list(), Vec::<String>::new());
    }

    #[test]
    fn rejects_bad_names() {
        let dir = tempfile::tempdir().unwrap();
        let store = SecretsStore::open(dir.path().join("secrets.toml")).unwrap();
        assert!(store.set("bad name", "v").is_err());
    }
}
