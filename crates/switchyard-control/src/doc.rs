// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! TOML document operations for the control plane.
//!
//! Every mutation parses the deployment source, edits one section, and
//! returns the rendered document. The caller validates the result with
//! `Runner::from_toml` before persisting, so these helpers only report
//! structural problems (bad names, missing tables), not routing semantics.

use std::collections::BTreeMap;
use std::fmt;

use serde_json::{Value, json};
use toml_edit::{DocumentMut, Item, table, value};

/// A structural problem editing the deployment document.
#[derive(Debug)]
pub struct DocError {
    message: String,
}

impl DocError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for DocError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for DocError {}

pub type DocResult<T> = Result<T, DocError>;

pub fn validate_section(section: &str) -> DocResult<()> {
    match section {
        "routes" | "llm_clients" | "targets" => Ok(()),
        other => Err(DocError::new(format!(
            "unknown section {other}; expected routes, llm_clients, or targets"
        ))),
    }
}

/// Bare TOML keys are safe to splice into table headers; reject anything else.
fn validate_name(name: &str, kind: &str) -> DocResult<()> {
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if valid {
        Ok(())
    } else {
        Err(DocError::new(format!(
            "{kind} {name:?} must be non-empty and contain only letters, digits, dashes, and underscores"
        )))
    }
}

pub fn parse(source: &str) -> DocResult<DocumentMut> {
    source
        .parse()
        .map_err(|error| DocError::new(format!("failed to parse TOML: {error}")))
}

fn section_table_mut<'a>(
    doc: &'a mut DocumentMut,
    section: &str,
    create: bool,
) -> Option<&'a mut toml_edit::Table> {
    let main = doc.as_table_mut();
    if main.get(section).is_none() && create {
        main.insert(section, table());
    }
    main.get_mut(section).and_then(Item::as_table_mut)
}

fn render_table(header: &str, table: &toml_edit::Table, out: &mut String) {
    out.push_str(&format!("[{header}]\n"));
    for (key, item) in table.iter() {
        match item {
            Item::Table(nested) => {
                out.push('\n');
                render_table(&format!("{header}.{key}"), nested, out);
            }
            Item::Value(value) => {
                // The value's display includes its parsed leading decor
                // (the space after `=`), which a fresh block must not carry.
                let rendered = value.to_string();
                out.push_str(&format!("{key} = {}\n", rendered.trim_start()));
            }
            _ => out.push_str(&format!("{key} = {item}\n")),
        }
    }
}

/// The `[section.name]` block as raw TOML text, ready for the console editor.
pub fn section_block(doc: &DocumentMut, section: &str, name: &str) -> DocResult<String> {
    validate_section(section)?;
    validate_name(name, "entry name")?;
    let table = doc
        .get(section)
        .and_then(Item::as_table)
        .and_then(|section| section.get(name).and_then(Item::as_table))
        .cloned()
        .ok_or_else(|| DocError::new(format!("{section}.{name} does not exist")))?;
    let mut out = String::new();
    render_table(&format!("{section}.{name}"), &table, &mut out);
    Ok(out)
}

/// Adds or replaces `[section.name]` from a raw TOML block.
///
/// `block` is the table body (`key = value` lines, optional nested tables).
/// A block that already starts with a table header is used as-is.
pub fn upsert_section(
    doc: &mut DocumentMut,
    section: &str,
    name: &str,
    block: &str,
) -> DocResult<()> {
    validate_section(section)?;
    validate_name(name, "entry name")?;
    let trimmed = block.trim();
    if trimmed.is_empty() {
        return Err(DocError::new("block is empty"));
    }
    let source = if trimmed.starts_with('[') {
        trimmed.to_string()
    } else {
        format!("[{section}.{name}]\n{block}")
    };
    let mut parsed = parse(&source)?;
    let main = parsed.as_table_mut();
    let entry = main
        .get_mut(section)
        .and_then(Item::as_table_mut)
        .and_then(|section| section.get(name).and_then(Item::as_table))
        .cloned()
        .ok_or_else(|| DocError::new(format!("block must define the [{section}.{name}] table")))?;
    let target = section_table_mut(doc, section, true)
        .ok_or_else(|| DocError::new(format!("section {section} is not a table")))?;
    target.insert(name, Item::Table(entry));
    Ok(())
}

pub fn remove_section_entry(doc: &mut DocumentMut, section: &str, name: &str) -> DocResult<()> {
    validate_section(section)?;
    validate_name(name, "entry name")?;
    let target = section_table_mut(doc, section, false)
        .ok_or_else(|| DocError::new(format!("section {section} does not exist")))?;
    if target.remove(name).is_none() {
        return Err(DocError::new(format!("{section}.{name} does not exist")));
    }
    Ok(())
}

pub fn remove_decision_model(doc: &mut DocumentMut, name: &str) -> DocResult<()> {
    validate_name(name, "decision model name")?;
    let target = section_table_mut(doc, "decision_models", false)
        .ok_or_else(|| DocError::new("decision model {name} does not exist"))?;
    if target.remove(name).is_none() {
        return Err(DocError::new(format!(
            "decision model {name} does not exist"
        )));
    }
    Ok(())
}

/// Adds or replaces `[decision_models.name]`.
pub fn upsert_decision_model(
    doc: &mut DocumentMut,
    name: &str,
    base_url: &str,
    model: &str,
    api_key_env: &str,
) -> DocResult<()> {
    validate_name(name, "decision model name")?;
    if base_url.trim().is_empty() || model.trim().is_empty() || api_key_env.trim().is_empty() {
        return Err(DocError::new(
            "base_url, model, and api_key_env must be non-empty",
        ));
    }
    let mut entry = table();
    let entry_table = entry.as_table_mut().expect("table() builds a table item");
    entry_table.insert("base_url", value(base_url.trim()));
    entry_table.insert("model", value(model.trim()));
    entry_table.insert("api_key_env", value(api_key_env.trim()));
    let target = section_table_mut(doc, "decision_models", true)
        .ok_or_else(|| DocError::new("decision_models is not a table"))?;
    target.insert(name, entry);
    Ok(())
}

fn route_decision(route: &toml_edit::Table) -> Option<Value> {
    if route.get("type").and_then(Item::as_str)? != "decision_model" {
        return None;
    }
    if let Some(name) = route.get("decision").and_then(Item::as_str) {
        Some(json!({ "name": name }))
    } else {
        Some(json!({
            "base_url": route.get("decision_base_url").and_then(Item::as_str),
            "model": route.get("decision_model").and_then(Item::as_str),
            "api_key_env": route.get("decision_api_key_env").and_then(Item::as_str),
        }))
    }
}

/// A saved `[decision_models.name]` entry as `(base_url, model, api_key_env)`.
pub fn decision_model_info(doc: &DocumentMut, name: &str) -> DocResult<(String, String, String)> {
    validate_name(name, "decision model name")?;
    let entry = doc
        .get("decision_models")
        .and_then(Item::as_table)
        .and_then(|section| section.get(name).and_then(Item::as_table))
        .ok_or_else(|| DocError::new(format!("decision model {name} does not exist")))?;
    let read = |key: &str| {
        entry
            .get(key)
            .and_then(Item::as_str)
            .unwrap_or_default()
            .to_string()
    };
    Ok((read("base_url"), read("model"), read("api_key_env")))
}

/// Route names that reference the decision model `name` by `decision = "name"`.
pub fn referencing_routes(doc: &DocumentMut, name: &str) -> Vec<String> {
    doc.get("routes")
        .and_then(Item::as_table)
        .map(|routes| {
            routes
                .iter()
                .filter(|(_, item)| {
                    item.as_table()
                        .and_then(|route| route.get("decision"))
                        .and_then(Item::as_str)
                        .is_some_and(|referenced| referenced == name)
                })
                .map(|(route_name, _)| route_name.to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// The whole deployment at a glance for the console.
pub fn summary(doc: &DocumentMut) -> Value {
    let mut clients = BTreeMap::new();
    if let Some(section) = doc.get("llm_clients").and_then(Item::as_table) {
        for (name, item) in section.iter() {
            if let Some(client) = item.as_table() {
                clients.insert(
                    name.to_string(),
                    json!({
                        "format": client.get("format").and_then(Item::as_str),
                        "base_url": client.get("base_url").and_then(Item::as_str),
                        "api_key_env": client.get("api_key_env").and_then(Item::as_str),
                        "forward_auth": client.get("forward_auth").and_then(Item::as_bool).unwrap_or(false),
                    }),
                );
            }
        }
    }
    let mut targets = BTreeMap::new();
    if let Some(section) = doc.get("targets").and_then(Item::as_table) {
        for (name, item) in section.iter() {
            if let Some(target) = item.as_table() {
                targets.insert(
                    name.to_string(),
                    json!({
                        "id": target.get("id").and_then(Item::as_str),
                        "llm_client": target.get("llm_client").and_then(Item::as_str),
                    }),
                );
            }
        }
    }
    let mut routes = BTreeMap::new();
    if let Some(section) = doc.get("routes").and_then(Item::as_table) {
        for (name, item) in section.iter() {
            if let Some(route) = item.as_table() {
                routes.insert(
                    name.to_string(),
                    json!({
                        "id": route.get("id").and_then(Item::as_str),
                        "type": route.get("type").and_then(Item::as_str),
                        "decision": route_decision(route),
                    }),
                );
            }
        }
    }
    let mut decision_models = BTreeMap::new();
    if let Some(section) = doc.get("decision_models").and_then(Item::as_table) {
        for (name, item) in section.iter() {
            if let Some(entry) = item.as_table() {
                decision_models.insert(
                    name.to_string(),
                    json!({
                        "base_url": entry.get("base_url").and_then(Item::as_str),
                        "model": entry.get("model").and_then(Item::as_str),
                        "api_key_env": entry.get("api_key_env").and_then(Item::as_str),
                        "routes": referencing_routes(doc, name),
                    }),
                );
            }
        }
    }
    json!({
        "schema_version": doc.get("schema_version").and_then(Item::as_integer),
        "fallback_client": doc.get("fallback_client").and_then(Item::as_str),
        "llm_clients": clients,
        "targets": targets,
        "routes": routes,
        "decision_models": decision_models,
    })
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

[decision_models.judge]
base_url = "https://api.example.test/v1"
model = "decision-model-preview"
api_key_env = "JUDGE_KEY"

[routes.decision]
id = "switchyard/decision"
type = "decision_model"
strong_target = "weak"
weak_target = "weak"
default_target = "weak"
decision = "judge"
"#;

    #[test]
    fn summary_lists_every_section() {
        let doc = parse(SOURCE).unwrap();
        let summary = summary(&doc);
        assert_eq!(summary["llm_clients"]["primary"]["format"], "openai_chat");
        assert_eq!(summary["targets"]["weak"]["id"], "weak/model");
        assert_eq!(summary["routes"]["decision"]["type"], "decision_model");
        assert_eq!(summary["routes"]["decision"]["decision"]["name"], "judge");
        assert_eq!(
            summary["decision_models"]["judge"]["model"],
            "decision-model-preview"
        );
        let used: Vec<&str> = summary["decision_models"]["judge"]["routes"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert_eq!(used, ["decision"]);
    }

    #[test]
    fn section_block_round_trips_through_upsert() {
        let doc = parse(SOURCE).unwrap();
        let block = section_block(&doc, "routes", "decision").unwrap();
        assert!(block.starts_with("[routes.decision]\n"));

        let mut updated = parse(SOURCE).unwrap();
        let new_block = block.replace(
            "decision = \"judge\"",
            "decision = \"judge\"\nconfidence_threshold = 0.6",
        );
        upsert_section(&mut updated, "routes", "decision", &new_block).unwrap();
        assert!(updated.to_string().contains("confidence_threshold = 0.6"));

        let block = section_block(&updated, "routes", "decision").unwrap();
        assert!(block.contains("confidence_threshold = 0.6"));
    }

    #[test]
    fn upsert_section_adds_a_missing_entry() {
        let mut doc = parse(SOURCE).unwrap();
        upsert_section(
            &mut doc,
            "targets",
            "strong",
            "id = \"strong/model\"\nllm_client = \"primary\"",
        )
        .unwrap();
        let rendered = doc.to_string();
        assert!(rendered.contains("[targets.strong]"));
        assert!(rendered.contains("id = \"strong/model\""));
    }

    #[test]
    fn upsert_section_rejects_blocks_with_the_wrong_header() {
        let mut doc = parse(SOURCE).unwrap();
        let error = upsert_section(&mut doc, "routes", "other", "[routes.decision]\nid = \"x\"")
            .unwrap_err();
        assert!(error.to_string().contains("[routes.other]"));
    }

    #[test]
    fn upsert_section_rejects_bad_names() {
        let mut doc = parse(SOURCE).unwrap();
        let error = upsert_section(&mut doc, "routes", "bad name", "id = \"x\"").unwrap_err();
        assert!(error.to_string().contains("bad name"));
    }

    #[test]
    fn remove_section_entry_reports_missing_entries() {
        let mut doc = parse(SOURCE).unwrap();
        remove_section_entry(&mut doc, "routes", "nope").unwrap_err();
        remove_section_entry(&mut doc, "routes", "decision").unwrap();
        let error = remove_section_entry(&mut doc, "routes", "decision").unwrap_err();
        assert!(error.to_string().contains("does not exist"));
    }

    #[test]
    fn remove_decision_model_requires_an_existing_entry() {
        let mut doc = parse(SOURCE).unwrap();
        assert!(referencing_routes(&doc, "judge") == vec!["decision".to_string()]);
        remove_decision_model(&mut doc, "judge").unwrap();
        let error = remove_decision_model(&mut doc, "judge").unwrap_err();
        assert!(error.to_string().contains("does not exist"));
    }

    #[test]
    fn upsert_decision_model_replaces_an_existing_entry() {
        let mut doc = parse(SOURCE).unwrap();
        upsert_decision_model(&mut doc, "judge", "https://api.other.test/v1", "m2", "K2").unwrap();
        assert!(
            doc.to_string()
                .contains("base_url = \"https://api.other.test/v1\"")
        );
    }
}
