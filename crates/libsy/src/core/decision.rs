// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Text-free structured decisions from a dedicated decision model.
//!
//! A [`DecisionCaller`] sends a compressed transcript and a fixed choice question
//! and gets one typed answer with a probability distribution in a single forward
//! pass. Latency and cost are independent of output length, which fits
//! high-frequency routing decisions.

use std::collections::BTreeMap;

use async_trait::async_trait;

use crate::Result;

/// Option names of the tier question every caller asks.
pub mod tier {
    /// The high-intelligence tier.
    pub const STRONG: &str = "strong";
    /// The cost-efficient tier.
    pub const WEAK: &str = "weak";
    /// Fallback option: neither tier clearly applies.
    pub const OTHER: &str = "other";
}

/// The typed answer to one choice question.
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionVerdict {
    /// Name of the selected option.
    pub choice: String,
    /// The API's confidence in the choice, in `[0.0, 1.0]`.
    pub confidence: f64,
    /// Probability of every option. Sums to 1.
    pub probabilities: BTreeMap<String, f64>,
}

/// Sends one transcript to a decision model and returns the typed answer.
#[async_trait]
pub trait DecisionCaller: Send + Sync {
    /// Classifies `state` and returns the answer to the tier question.
    ///
    /// Errors on transport or response failures; the caller decides how to
    /// fall back.
    async fn classify(&self, state: &str) -> Result<DecisionVerdict>;
}
