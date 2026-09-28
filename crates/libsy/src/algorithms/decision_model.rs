// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! A dedicated decision model picks the tier for each request.
//!
//! On every trigger the router sends a compressed transcript to a decision model
//! that answers a fixed choice question in one forward pass — no text generation.
//! The chosen option names are [`tier`](crate::core::decision::tier). Low-confidence
//! choices, the `other` option, and failed calls all route to the configured
//! default target.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use switchyard_protocol::{Category, ContentBlock, ModelId, Request, Response, Role};

use super::fall_through::FallThrough;
use super::util::affinity::{ClassifyTrigger, affinity_router, is_user_turn};
use crate::core::algorithm::{Algorithm, Driver, RoutingOutcome};
use crate::core::classifier::{Classification, Classifier, Score};
use crate::core::decision::{DecisionCaller, DecisionVerdict, tier};
use crate::{LibsyError, Result};

/// Telemetry label for this algorithm's spans, metrics, and logs.
const ALGORITHM_NAME: &str = "decision_model_router";

/// Most characters of the instruction text sent to the decision model.
///
/// Long agent system prompts are mostly tool documentation and behavioral
/// rules: they dilute the difficulty signal, and live probing shows a longer
/// prompt lowers P(strong) on hard tasks. A few hundred characters keep the
/// "what kind of agent is this" context without the dilution.
const SYSTEM_MAX_CHARS: usize = 300;
/// Most characters of one conversation message sent to the decision model.
const MESSAGE_MAX_CHARS: usize = 2000;
/// Most characters of the final message — the request being routed — sent.
const LAST_MESSAGE_MAX_CHARS: usize = 8000;
/// Tool-result preview length. The opening of a result (test output, an error,
/// a file snippet) is a strong difficulty signal; the rest is noise.
const TOOL_RESULT_PREVIEW_CHARS: usize = 200;
/// Trailing turns the decision model sees when `recent_turn_window` is unset.
const DEFAULT_RECENT_TURNS: usize = 2;
/// Default lowest probability that still routes the decision model's pick.
pub const DEFAULT_CONFIDENCE_THRESHOLD: f64 = 0.7;

/// Construction settings for [`DecisionModelRouter`].
#[derive(Clone)]
pub struct DecisionModelRouterConfig {
    /// Performs the decision model call.
    pub caller: Arc<dyn DecisionCaller>,
    /// The high-intelligence tier.
    pub strong_target: ModelId,
    /// The cost-efficient tier.
    pub weak_target: ModelId,
    /// Target used when the decision is low-confidence or the call fails.
    pub default_target: ModelId,
    /// Lowest probability for a tier option that still routes that tier. Below
    /// it the router uses `default_target`, from 0 to 1. `None` trusts the
    /// decision model's own pick: the chosen option routes directly, and only
    /// `other` falls to `default_target`.
    pub confidence_threshold: Option<f64>,
    /// How often the decision model runs.
    pub classify_trigger: ClassifyTrigger,
    /// Reuses the session's target by hashing the first user message when no
    /// session ID is available. Needs a retaining trigger.
    pub message_hash_fallback: bool,
    /// Trailing turns the decision model sees. Defaults to two.
    pub recent_turn_window: Option<usize>,
}

impl std::fmt::Debug for DecisionModelRouterConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecisionModelRouterConfig")
            .field("strong_target", &self.strong_target)
            .field("weak_target", &self.weak_target)
            .field("default_target", &self.default_target)
            .field("confidence_threshold", &self.confidence_threshold)
            .field("classify_trigger", &self.classify_trigger)
            .field("message_hash_fallback", &self.message_hash_fallback)
            .field("recent_turn_window", &self.recent_turn_window)
            .field("caller", &format_args!("<DecisionCaller>"))
            .finish()
    }
}

impl DecisionModelRouterConfig {
    /// Checks the settings are usable.
    ///
    /// # Errors
    ///
    /// Returns an error when the targets are not distinct or a knob is out of range.
    pub fn validate(&self) -> Result<()> {
        if self.strong_target == self.weak_target {
            return Err(LibsyError::AlgorithmError {
                message: "strong_target and weak_target must be different models".to_string(),
            });
        }
        if let Some(threshold) = self.confidence_threshold
            && (threshold.is_nan() || !(0.0..=1.0).contains(&threshold))
        {
            return Err(LibsyError::AlgorithmError {
                message: format!("confidence_threshold must be in [0.0, 1.0], got {threshold}"),
            });
        }
        if self.message_hash_fallback && self.classify_trigger == ClassifyTrigger::EveryRequest {
            return Err(LibsyError::AlgorithmError {
                message:
                    "message_hash_fallback requires classify_trigger = new_session or user_turn"
                        .to_string(),
            });
        }
        Ok(())
    }
}

/// Routes between two tiers from a dedicated decision model's typed verdict.
pub struct DecisionModelRouter {
    route: FallThrough<()>,
}

impl DecisionModelRouter {
    /// Builds the router described by `config`.
    ///
    /// # Errors
    ///
    /// Returns an error when the settings fail [`validate`](Self::config).
    pub fn new(config: DecisionModelRouterConfig) -> Result<Self> {
        config.validate()?;
        let window = config
            .recent_turn_window
            .unwrap_or(DEFAULT_RECENT_TURNS)
            .max(1);
        // Affinity comes first so a retained assignment short-circuits the call.
        let mut route = FallThrough::new().with_name(ALGORITHM_NAME);
        if let Some(affinity) =
            affinity_router(config.classify_trigger, config.message_hash_fallback)
        {
            // Both roles must share one `Arc` so the classifier reads what the processor wrote.
            route = route
                .with_processor(affinity.clone())
                .with_classifier(affinity.clone());
        }
        route = route.with_classifier(Arc::new(DecisionModelClassifier {
            caller: config.caller,
            strong: config.strong_target,
            weak: config.weak_target,
            default: config.default_target,
            threshold: config.confidence_threshold,
            window,
        }));
        Ok(Self { route })
    }
}

#[async_trait]
impl Algorithm for DecisionModelRouter {
    fn name(&self) -> &str {
        ALGORITHM_NAME
    }

    async fn route(self: Arc<Self>, driver: Driver, request: Request) -> Result<RoutingOutcome> {
        self.route.execute(driver, request).await
    }
}

/// Scores the request by asking the decision model which tier should serve it.
struct DecisionModelClassifier {
    caller: Arc<dyn DecisionCaller>,
    strong: ModelId,
    weak: ModelId,
    default: ModelId,
    threshold: Option<f64>,
    window: usize,
}

#[async_trait]
impl Classifier for DecisionModelClassifier {
    async fn score(
        &self,
        _state: &mut (),
        request: &mut Request,
        driver: &Driver,
    ) -> Result<(Classification, Option<Response>)> {
        let state = build_state(request, self.window);
        let verdict = match self.caller.classify(&state).await {
            Ok(verdict) => verdict,
            Err(error) => {
                tracing::warn!(
                    algorithm = ALGORITHM_NAME,
                    %error,
                    "decision model call failed; routing to the default target"
                );
                driver.set_evidence_if_empty(json!({
                    "source": "decision_model",
                    "result": "failed",
                }));
                return Ok((self.default_classification(), None));
            }
        };
        let (target, category) = pick_tier(&verdict, self.threshold, &self.strong, &self.weak);
        driver.set_evidence_if_empty(json!({
            "source": "decision_model",
            "choice": verdict.choice,
            "confidence": verdict.confidence,
            "probabilities": verdict.probabilities,
            "state_chars": state.chars().count(),
        }));
        tracing::info!(
            algorithm = ALGORITHM_NAME,
            choice = %verdict.choice,
            confidence = verdict.confidence,
            probabilities = %format!("{:?}", verdict.probabilities),
            state_chars = state.chars().count(),
            routed = target.is_some(),
            "decision model verdict"
        );
        let score = match (target, category) {
            (Some(target), Some(category)) => Score {
                target,
                confidence: verdict.confidence,
                category: Some(category),
            },
            (None, None) => Score {
                target: self.default.clone(),
                confidence: 0.0,
                category: Some(Category::Any),
            },
            _ => unreachable!("pick_tier returns either both or neither"),
        };
        Ok((Classification::Scores(vec![score]), None))
    }
}

impl DecisionModelClassifier {
    /// The routing choice that replaces an unusable verdict.
    fn default_classification(&self) -> Classification {
        Classification::Scores(vec![Score {
            target: self.default.clone(),
            confidence: 0.0,
            category: Some(Category::Any),
        }])
    }
}

/// Maps one verdict to a tier, or to nothing when the router keeps the default.
///
/// With a threshold, routes on the option probabilities, not the API's
/// reported confidence: the confidence does not track the distribution (a
/// correct strong pick can report 0.44 while P(strong) is 0.68), and `other`
/// carries a 0.2-0.4 baseline in agent transcripts that would veto clear picks.
/// Without one, trusts the model's own pick: `choice` routes directly and only
/// `other` falls to the default target.
fn pick_tier(
    verdict: &DecisionVerdict,
    threshold: Option<f64>,
    strong: &ModelId,
    weak: &ModelId,
) -> (Option<ModelId>, Option<Category>) {
    let Some(threshold) = threshold else {
        return match verdict.choice.as_str() {
            tier::STRONG => (Some(strong.clone()), Some(Category::Capable)),
            tier::WEAK => (Some(weak.clone()), Some(Category::Efficient)),
            _ => (None, None),
        };
    };
    let p_strong = verdict
        .probabilities
        .get(tier::STRONG)
        .copied()
        .unwrap_or(0.0);
    let p_weak = verdict
        .probabilities
        .get(tier::WEAK)
        .copied()
        .unwrap_or(0.0);
    if p_strong >= threshold {
        (Some(strong.clone()), Some(Category::Capable))
    } else if p_weak >= threshold {
        (Some(weak.clone()), Some(Category::Efficient))
    } else {
        (None, None)
    }
}

/// Compresses the request into the plain-text transcript the decision model sees.
///
/// Keeps the instruction text, the trailing user turns (including their tool
/// calls and results), and the names of tool calls. Drops tool result bodies,
/// media, and reasoning: bulky blocks that carry little signal about how hard
/// the current request is.
///
/// The window counts user turns, not messages: inside an agent loop the last
/// messages are often tool continuations, and a pure message tail can drop the
/// request the decision is actually about.
fn build_state(request: &Request, window: usize) -> String {
    let llm = &request.llm_request;
    let messages = &llm.messages;
    let turn_starts: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, message)| is_user_turn(message))
        .map(|(index, _)| index)
        .collect();
    // The newest user turn carries the request being routed; give it the larger cap.
    let request_turn = turn_starts.last().copied();
    let start = if turn_starts.is_empty() {
        messages.len().saturating_sub(window)
    } else {
        let first = turn_starts.len().saturating_sub(window);
        turn_starts[first]
    };
    let mut out = String::new();
    if let Some(instructions) = instruction_text(&llm.instructions) {
        out.push_str("system: ");
        out.push_str(&instructions);
        out.push('\n');
    }
    for (index, message) in messages.iter().enumerate().skip(start) {
        out.push_str(role_prefix(message.role));
        out.push_str(&message_text(message, Some(index) == request_turn));
        out.push('\n');
    }
    out
}

/// Concatenates the instruction blocks, capped.
fn instruction_text(instructions: &[switchyard_protocol::InstructionBlock]) -> Option<String> {
    let text = instructions
        .iter()
        .flat_map(|block| {
            block
                .content
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let trimmed = truncate(&text, SYSTEM_MAX_CHARS);
    (!trimmed.trim().is_empty()).then_some(trimmed)
}

/// One message's text view, with placeholders for dropped blocks.
fn message_text(message: &switchyard_protocol::Message, is_last: bool) -> String {
    let cap = if is_last {
        LAST_MESSAGE_MAX_CHARS
    } else {
        MESSAGE_MAX_CHARS
    };
    let mut parts = Vec::new();
    for block in &message.content {
        match block {
            ContentBlock::Text { text } | ContentBlock::Refusal { text } => {
                parts.push(truncate(text, cap))
            }
            ContentBlock::Reasoning { .. } => {}
            ContentBlock::Image { .. } => parts.push("[image]".to_string()),
            ContentBlock::Audio { .. } => parts.push("[audio]".to_string()),
            ContentBlock::Video { .. } => parts.push("[video]".to_string()),
            ContentBlock::File { .. } => parts.push("[file]".to_string()),
            ContentBlock::ToolCall(call) => parts.push(format!("[tool call: {}]", call.name)),
            ContentBlock::ToolResult(result) => parts.push(tool_result_preview(result)),
            ContentBlock::Unknown { .. } => {}
        }
    }
    truncate(&parts.join(" "), cap)
}

/// A tool result's preview: its opening, which carries the difficulty signal.
fn tool_result_preview(result: &switchyard_protocol::ToolResult) -> String {
    let text = result
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ");
    if text.trim().is_empty() {
        "[tool result: omitted]".to_string()
    } else {
        format!(
            "[tool result: {}]",
            truncate(&text, TOOL_RESULT_PREVIEW_CHARS)
        )
    }
}

fn role_prefix(role: Role) -> &'static str {
    match role {
        Role::User => "user: ",
        Role::Assistant => "assistant: ",
        Role::Tool => "tool: ",
        Role::System | Role::Developer => "system: ",
    }
}

/// Truncates `text` to at most `max_chars`, marking the cut.
fn truncate(text: &str, max_chars: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        return text.to_string();
    }
    let mut out: String = chars[..max_chars].iter().collect();
    out.push_str("…[truncated]");
    out
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use parking_lot::Mutex;
    use switchyard_protocol::{
        InstructionBlock, LlmRequest, Metadata, Role, ToolCall, ToolResult, text_request,
    };

    use super::*;
    use crate::core::testing::{echo, test_drive_with_models};

    struct MockCaller {
        verdict: Mutex<Option<DecisionVerdict>>,
        error: Mutex<Option<String>>,
        last_state: Mutex<Option<String>>,
        calls: Mutex<u32>,
    }

    impl MockCaller {
        /// A verdict that concentrates its probability on `choice`, the way a
        /// confident decision model answer does.
        fn verdict(choice: &str, confidence: f64) -> DecisionVerdict {
            DecisionVerdict {
                choice: choice.to_string(),
                confidence,
                probabilities: vec![(choice.to_string(), confidence)].into_iter().collect(),
            }
        }
    }

    #[async_trait]
    impl DecisionCaller for MockCaller {
        async fn classify(&self, state: &str) -> Result<DecisionVerdict> {
            *self.last_state.lock() = Some(state.to_string());
            *self.calls.lock() += 1;
            match self.error.lock().clone() {
                Some(message) => Err(LibsyError::AlgorithmError { message }),
                None => Ok(self.verdict.lock().clone().expect("verdict set")),
            }
        }
    }

    fn mock(verdict: Option<DecisionVerdict>, error: Option<String>) -> Arc<MockCaller> {
        Arc::new(MockCaller {
            verdict: Mutex::new(verdict),
            error: Mutex::new(error),
            last_state: Mutex::new(None),
            calls: Mutex::new(0),
        })
    }

    fn config(caller: Arc<MockCaller>) -> DecisionModelRouterConfig {
        DecisionModelRouterConfig {
            caller,
            strong_target: ModelId::from("strong"),
            weak_target: ModelId::from("weak"),
            default_target: ModelId::from("weak"),
            confidence_threshold: Some(DEFAULT_CONFIDENCE_THRESHOLD),
            classify_trigger: ClassifyTrigger::default(),
            message_hash_fallback: false,
            recent_turn_window: None,
        }
    }

    fn request_with_session(prompt: &str, session: &str) -> Request {
        Request {
            llm_request: text_request(None, prompt),
            raw_request: None,
            metadata: Some(Metadata {
                session_id: Some(session.to_string()),
                ..Default::default()
            }),
        }
    }

    fn models() -> HashMap<Category, Vec<ModelId>> {
        let mut map = HashMap::new();
        map.insert(Category::Capable, vec![ModelId::from("strong")]);
        map.insert(Category::Efficient, vec![ModelId::from("weak")]);
        map.insert(
            Category::Any,
            vec![ModelId::from("strong"), ModelId::from("weak")],
        );
        map
    }

    async fn route(caller: Arc<MockCaller>, prompt: &str) -> ModelId {
        let router = Arc::new(DecisionModelRouter::new(config(caller)).unwrap());
        let (selected, _) =
            test_drive_with_models(router, request_with_session(prompt, "s1"), models(), echo())
                .await
                .unwrap();
        selected
    }

    #[tokio::test]
    async fn a_confident_strong_verdict_routes_to_the_strong_tier() {
        let caller = mock(Some(MockCaller::verdict(tier::STRONG, 0.9)), None);
        let selected = route(caller.clone(), "debug this race condition").await;
        assert_eq!(selected.as_str(), "strong");
        assert_eq!(*caller.calls.lock(), 1);
    }

    #[tokio::test]
    async fn a_confident_weak_verdict_routes_to_the_weak_tier() {
        let caller = mock(Some(MockCaller::verdict(tier::WEAK, 0.95)), None);
        let selected = route(caller, "rename this variable").await;
        assert_eq!(selected.as_str(), "weak");
    }

    #[tokio::test]
    async fn a_low_confidence_verdict_routes_to_the_default_target() {
        let caller = mock(Some(MockCaller::verdict(tier::STRONG, 0.4)), None);
        let selected = route(caller, "debug this").await;
        assert_eq!(selected.as_str(), "weak");
    }

    #[tokio::test]
    async fn an_other_verdict_routes_to_the_default_target() {
        let caller = mock(Some(MockCaller::verdict(tier::OTHER, 0.99)), None);
        let selected = route(caller, "maybe?").await;
        assert_eq!(selected.as_str(), "weak");
    }

    #[tokio::test]
    async fn without_a_threshold_the_models_pick_routes_directly() {
        let mut caller = MockCaller::verdict(tier::STRONG, 0.3);
        // The model picks strong with a probability no threshold would clear.
        caller.probabilities.insert(tier::STRONG.to_string(), 0.4);
        caller.probabilities.insert(tier::OTHER.to_string(), 0.6);
        let caller = mock(Some(caller), None);
        let mut config = config(caller);
        config.confidence_threshold = None;
        let router = Arc::new(DecisionModelRouter::new(config).unwrap());
        let (selected, _) = test_drive_with_models(
            router,
            request_with_session("debug this", "s1"),
            models(),
            echo(),
        )
        .await
        .unwrap();
        assert_eq!(selected.as_str(), "strong");
    }

    #[tokio::test]
    async fn without_a_threshold_other_still_routes_to_the_default_target() {
        let caller = mock(Some(MockCaller::verdict(tier::OTHER, 0.5)), None);
        let mut config = config(caller);
        config.confidence_threshold = None;
        let router = Arc::new(DecisionModelRouter::new(config).unwrap());
        let (selected, _) = test_drive_with_models(
            router,
            request_with_session("maybe?", "s1"),
            models(),
            echo(),
        )
        .await
        .unwrap();
        assert_eq!(selected.as_str(), "weak");
    }

    #[tokio::test]
    async fn a_failed_call_falls_open_to_the_default_target() {
        let caller = mock(
            Some(MockCaller::verdict(tier::STRONG, 0.99)),
            Some("boom".to_string()),
        );
        let selected = route(caller, "debug this").await;
        assert_eq!(selected.as_str(), "weak");
    }

    #[tokio::test]
    async fn the_transcript_keeps_text_and_tool_names_but_drops_tool_results() {
        let caller = mock(Some(MockCaller::verdict(tier::WEAK, 0.9)), None);
        let llm = LlmRequest {
            instructions: vec![InstructionBlock {
                role: Role::System,
                content: vec![ContentBlock::Text {
                    text: "You are a coding agent.".to_string(),
                }],
            }],
            messages: vec![
                switchyard_protocol::Message {
                    role: Role::User,
                    content: vec![ContentBlock::Text {
                        text: "look at the build log".to_string(),
                    }],
                },
                switchyard_protocol::Message {
                    role: Role::Assistant,
                    content: vec![
                        ContentBlock::ToolCall(ToolCall {
                            id: "c1".to_string(),
                            name: "run_build".to_string(),
                            arguments: serde_json::json!({}),
                        }),
                        ContentBlock::Reasoning {
                            text: "internal thinking".to_string(),
                            signature: None,
                            details: vec![],
                        },
                    ],
                },
                switchyard_protocol::Message {
                    role: Role::Tool,
                    content: vec![ContentBlock::ToolResult(ToolResult {
                        tool_call_id: "c1".to_string(),
                        content: vec![ContentBlock::Text {
                            text: format!("failed at line 42 \n{}", "x".repeat(500)),
                        }],
                        is_error: None,
                    })],
                },
                switchyard_protocol::Message::text(Role::User, "now fix the failing test"),
            ],
            ..LlmRequest::default()
        };
        let request = Request {
            llm_request: llm,
            raw_request: None,
            metadata: Some(Metadata {
                session_id: Some("s1".to_string()),
                ..Default::default()
            }),
        };
        let mut config = config(caller.clone());
        config.recent_turn_window = Some(3);
        let router = Arc::new(DecisionModelRouter::new(config).unwrap());
        test_drive_with_models(router, request, models(), echo())
            .await
            .unwrap();
        let state = caller.last_state.lock().clone().unwrap();
        assert!(state.contains("system: You are a coding agent."));
        assert!(state.contains("[tool call: run_build]"));
        // The tool result's opening is kept as a preview; its tail is dropped.
        assert!(state.contains("[tool result: failed at line 42"));
        assert!(!state.contains(&"x".repeat(200)));
        assert!(state.contains("now fix the failing test"));
        assert!(!state.contains("internal thinking"));
    }

    #[tokio::test]
    async fn a_tool_continuation_keeps_the_user_request_in_the_transcript() {
        let caller = mock(Some(MockCaller::verdict(tier::STRONG, 0.9)), None);
        let llm = LlmRequest {
            messages: vec![
                switchyard_protocol::Message::text(
                    Role::User,
                    "the deadlock in our cache layer is still happening under load",
                ),
                switchyard_protocol::Message {
                    role: Role::Assistant,
                    content: vec![ContentBlock::ToolCall(ToolCall {
                        id: "c1".to_string(),
                        name: "read_file".to_string(),
                        arguments: serde_json::json!({}),
                    })],
                },
                switchyard_protocol::Message {
                    role: Role::Tool,
                    content: vec![ContentBlock::ToolResult(ToolResult {
                        tool_call_id: "c1".to_string(),
                        content: vec![ContentBlock::Text {
                            text: "cache.go contents".to_string(),
                        }],
                        is_error: None,
                    })],
                },
                // Anthropic-style tool continuation: a user message of pure tool results.
                switchyard_protocol::Message {
                    role: Role::User,
                    content: vec![ContentBlock::ToolResult(ToolResult {
                        tool_call_id: "c1".to_string(),
                        content: vec![ContentBlock::Text {
                            text: "ok".to_string(),
                        }],
                        is_error: None,
                    })],
                },
            ],
            ..LlmRequest::default()
        };
        let request = Request {
            llm_request: llm,
            raw_request: None,
            metadata: Some(Metadata {
                session_id: Some("s3".to_string()),
                ..Default::default()
            }),
        };
        let router = Arc::new(DecisionModelRouter::new(config(caller.clone())).unwrap());
        test_drive_with_models(router, request, models(), echo())
            .await
            .unwrap();
        let state = caller.last_state.lock().clone().unwrap();
        assert!(state.contains("the deadlock in our cache layer"));
        assert!(state.contains("[tool call: read_file]"));
        assert!(state.contains("[tool result: cache.go contents]"));
    }

    #[tokio::test]
    async fn validation_rejects_identical_tiers() {
        let mut config = config(mock(Some(MockCaller::verdict(tier::WEAK, 0.9)), None));
        config.weak_target = ModelId::from("strong");
        assert!(DecisionModelRouter::new(config).is_err());
    }

    #[tokio::test]
    async fn the_user_turn_trigger_retains_the_pick_across_tool_continuations() {
        let caller = mock(Some(MockCaller::verdict(tier::STRONG, 0.9)), None);
        let mut config = config(caller.clone());
        config.classify_trigger = ClassifyTrigger::UserTurn;
        let router = Arc::new(DecisionModelRouter::new(config).unwrap());

        let (first, _) = test_drive_with_models(
            router.clone(),
            request_with_session("debug this race", "s2"),
            models(),
            echo(),
        )
        .await
        .unwrap();
        assert_eq!(first.as_str(), "strong");
        assert_eq!(*caller.calls.lock(), 1);

        // A tool continuation carries no new user turn: the retained target
        // short-circuits the call.
        let continuation = Request {
            llm_request: LlmRequest {
                messages: vec![
                    switchyard_protocol::Message::text(Role::User, "debug this race"),
                    switchyard_protocol::Message {
                        role: Role::Tool,
                        content: vec![ContentBlock::ToolResult(ToolResult {
                            tool_call_id: "c1".to_string(),
                            content: vec![ContentBlock::Text {
                                text: "ok".to_string(),
                            }],
                            is_error: None,
                        })],
                    },
                ],
                ..LlmRequest::default()
            },
            raw_request: None,
            metadata: Some(Metadata {
                session_id: Some("s2".to_string()),
                ..Default::default()
            }),
        };
        let (second, _) = test_drive_with_models(router, continuation, models(), echo())
            .await
            .unwrap();
        assert_eq!(second.as_str(), "strong");
        assert_eq!(*caller.calls.lock(), 1);
    }
}
