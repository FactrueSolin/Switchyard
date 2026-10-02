// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Control plane for Switchyard decision-model routing deployments.
//!
//! One process runs both planes. The data plane is the unmodified
//! `switchyard-server` router. The control plane is a management API plus an
//! embedded web console that edits the same TOML deployment file the runner
//! already parses, validates every change with `Runner::from_toml`, and
//! hot-swaps the route table without a restart.

pub mod api;
pub mod doc;
pub mod probe;
pub mod secrets;
pub mod store;

pub use api::{AdminState, admin_router, web_ui};
pub use secrets::SecretsStore;
pub use store::ConfigStore;
