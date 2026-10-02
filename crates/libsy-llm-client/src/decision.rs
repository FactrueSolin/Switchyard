// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! HTTP client for a decision model: one POST returns typed choices with
//! probability distributions in a single forward pass, without generating text.
//!
//! The wire protocol is the provider's System One `/systemone` endpoint,
//! reached by appending `/systemone` to an OpenAI-compatible `v1` root.

use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};
use switchyard_libsy::{DecisionCaller, DecisionVerdict, LibsyError, Result, tier};

/// The fixed choice question every call asks.
const TIER_INSTRUCTIONS: &str = "Which model should serve the latest user request?";
const TIER_STRONG_CRITERION: &str = "Needs deep multi-step reasoning, precise long-range dependencies, or the session is mid-way through a hard problem (debugging, architecture, subtle correctness)";
const TIER_WEAK_CRITERION: &str =
    "Routine and low-risk: lookups, chat, simple edits, boilerplate, mechanical transformations";
const TIER_OTHER_CRITERION: &str = "Mixed signals; neither clearly applies";

/// A decision model wire failure.
#[derive(Debug)]
struct DecisionModelError(String);

impl std::fmt::Display for DecisionModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DecisionModelError {}

fn decision_error(message: impl Into<String>) -> LibsyError {
    LibsyError::external("decision model", DecisionModelError(message.into()))
}

/// Calls a decision model's `systemone` endpoint over HTTP.
pub struct DecisionModelClient {
    client: reqwest::Client,
    url: String,
    api_key: Option<String>,
    model: String,
    timeout: Option<Duration>,
}

impl std::fmt::Debug for DecisionModelClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecisionModelClient")
            .field("url", &self.url)
            .field("model", &self.model)
            .field("timeout", &self.timeout)
            .finish()
    }
}

impl DecisionModelClient {
    /// Builds a client for the `systemone` endpoint under `base_url`.
    ///
    /// `base_url` is the provider's OpenAI-compatible `v1` root, such as
    /// `https://api.typesafe.ai/v1` or
    /// `https://{workspace}.cn-beijing.maas.aliyuncs.com/compatible-mode/v1`.
    ///
    /// # Errors
    ///
    /// Returns an error when the HTTP client cannot be built.
    pub fn new(
        base_url: &str,
        model: impl Into<String>,
        api_key: Option<String>,
        timeout: Option<Duration>,
    ) -> Result<Self> {
        let client = reqwest::Client::builder()
            .build()
            .map_err(|error| decision_error(format!("build HTTP client: {error}")))?;
        let url = format!("{}/systemone", base_url.trim_end_matches('/'));
        Ok(Self {
            client,
            url,
            api_key,
            model: model.into(),
            timeout,
        })
    }
}

#[async_trait]
impl DecisionCaller for DecisionModelClient {
    async fn classify(&self, state: &str) -> Result<DecisionVerdict> {
        let body = json!({
            "model": self.model,
            "state": state,
            "questions": {
                "tier": {
                    "type": "choice",
                    "instructions": TIER_INSTRUCTIONS,
                    "criteria": {
                        tier::STRONG: TIER_STRONG_CRITERION,
                        tier::WEAK: TIER_WEAK_CRITERION,
                        tier::OTHER: TIER_OTHER_CRITERION,
                    }
                }
            }
        });
        let mut request = self.client.post(&self.url).json(&body);
        if let Some(api_key) = &self.api_key {
            request = request.bearer_auth(api_key);
        }
        let request = match self.timeout {
            Some(timeout) => request.timeout(timeout),
            None => request,
        };
        let response = request
            .send()
            .await
            .map_err(|error| decision_error(format!("call failed: {error}")))?;
        let status = response.status();
        let value: Value = response
            .json()
            .await
            .map_err(|error| decision_error(format!("decode response: {error}")))?;
        if !status.is_success() {
            return Err(decision_error(format!("status {status}: {value}")));
        }
        let tier = value
            .pointer("/answers/tier")
            .ok_or_else(|| decision_error("answers.tier missing"))?;
        let choice = tier
            .get("choice")
            .and_then(Value::as_str)
            .ok_or_else(|| decision_error("answers.tier.choice missing"))?
            .to_string();
        let confidence = tier
            .get("confidence")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let probabilities = tier
            .get("probabilities")
            .and_then(Value::as_object)
            .map(|probabilities| {
                probabilities
                    .iter()
                    .filter_map(|(option, probability)| {
                        probability.as_f64().map(|value| (option.clone(), value))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let usage = value.pointer("/usage");
        let latency_ms = value.pointer("/latency_ms").and_then(Value::as_f64);
        if usage.is_some() {
            tracing::debug!(?usage, latency_ms, "decision model answered");
        }
        Ok(DecisionVerdict {
            choice,
            confidence,
            probabilities,
        })
    }
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    async fn client(server: &MockServer) -> DecisionModelClient {
        DecisionModelClient::new(
            server.uri().as_str(),
            "decision-model-preview",
            Some("key".into()),
            None,
        )
        .unwrap()
    }

    fn ok_response() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "model": "decision-model-preview",
            "request_id": "r1",
            "answers": {
                "tier": {
                    "type": "choice",
                    "choice": "strong",
                    "confidence": 0.9,
                    "probabilities": {"strong": 0.94, "weak": 0.05, "other": 0.01}
                }
            },
            "usage": {"input_tokens": 125},
            "latency_ms": 52.9
        }))
    }

    #[tokio::test]
    async fn it_posts_the_tier_question_and_parses_the_answer() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/systemone"))
            .and(body_json(json!({
                "model": "decision-model-preview",
                "state": "user: debug this",
                "questions": {
                    "tier": {
                        "type": "choice",
                        "instructions": TIER_INSTRUCTIONS,
                        "criteria": {
                            "strong": TIER_STRONG_CRITERION,
                            "weak": TIER_WEAK_CRITERION,
                            "other": TIER_OTHER_CRITERION
                        }
                    }
                }
            })))
            .respond_with(ok_response())
            .expect(1)
            .mount(&server)
            .await;

        let verdict = client(&server)
            .await
            .classify("user: debug this")
            .await
            .unwrap();
        assert_eq!(verdict.choice, "strong");
        assert_eq!(verdict.confidence, 0.9);
        assert!((verdict.probabilities["strong"] - 0.94).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn a_non_success_status_is_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/systemone"))
            .respond_with(
                ResponseTemplate::new(429)
                    .set_body_json(json!({"error": {"message": "throttled"}})),
            )
            .mount(&server)
            .await;

        let error = client(&server).await.classify("state").await.unwrap_err();
        assert!(error.to_string().contains("429"));
    }

    #[tokio::test]
    async fn a_missing_tier_answer_is_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/systemone"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"answers": {}})))
            .mount(&server)
            .await;

        assert!(client(&server).await.classify("state").await.is_err());
    }
}
