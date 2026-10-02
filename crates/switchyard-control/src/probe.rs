// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Connection test for a decision-model endpoint: one real `systemone` call
//! with a trivial transcript.

use std::collections::BTreeMap;
use std::time::Duration;

use libsy::DecisionCaller;
use serde::Serialize;
use switchyard_llm_client::DecisionModelClient;

/// Probe deadline. A healthy decision call takes about 50 ms; anything slower
/// is already useless for routing.
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);

/// A trivial, clearly-routine request. The verdict is not the point; reaching
/// the model and parsing its answer is.
const PROBE_STATE: &str = "user: What is the capital of France?";

/// The decision model's verdict for the probe call.
#[derive(Debug, Clone, Serialize)]
pub struct ProbeOutcome {
    pub choice: String,
    pub confidence: f64,
    pub probabilities: BTreeMap<String, f64>,
}

/// Calls `{base_url}/systemone` once and returns the verdict.
pub async fn probe(
    base_url: &str,
    model: &str,
    api_key: Option<String>,
) -> Result<ProbeOutcome, String> {
    let caller = DecisionModelClient::new(base_url, model, api_key, Some(PROBE_TIMEOUT))
        .map_err(|error| format!("build probe client: {error}"))?;
    let verdict = caller
        .classify(PROBE_STATE)
        .await
        .map_err(|error| error.to_string())?;
    Ok(ProbeOutcome {
        choice: verdict.choice,
        confidence: verdict.confidence,
        probabilities: verdict.probabilities,
    })
}
