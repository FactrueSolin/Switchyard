// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! CLI entrypoint for the combined data-plane + control-plane process.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use clap::Parser;
use switchyard_runner::Runner;
use switchyard_server::{
    BoundServer, DEFAULT_GRACEFUL_SHUTDOWN_TIMEOUT, DEFAULT_LISTEN_BACKLOG, ServerRunOptions,
    ServerState, TlsOptions,
};
use tokio::net::TcpListener;

use switchyard_control::api::{AdminState, admin_router, web_ui};
use switchyard_control::secrets::SecretsStore;
use switchyard_control::store::ConfigStore;

const DEFAULT_HOST: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
const DEFAULT_PORT: u16 = 4000;
const DEFAULT_ADMIN_PORT: u16 = 4001;

/// Command-line arguments accepted by the control-plane binary.
#[derive(Debug, Parser)]
#[command(
    name = "switchyard-control",
    about = "Serve a Switchyard deployment with an admin API and web console",
    version
)]
pub struct ControlArgs {
    /// TOML file defining LLM clients, decision models, targets, and routes.
    #[arg(long, value_name = "PATH")]
    config: PathBuf,

    /// Secrets file (TOML) backing the api_key_env values. Defaults to
    /// `secrets.toml` beside the config file.
    #[arg(long, value_name = "PATH")]
    secrets: Option<PathBuf>,

    /// Host address for the data plane.
    #[arg(long, default_value_t = DEFAULT_HOST)]
    host: IpAddr,

    /// Port for the data plane.
    #[arg(short, long, default_value_t = DEFAULT_PORT)]
    port: u16,

    /// Host address for the admin API and web console.
    #[arg(long, default_value_t = DEFAULT_HOST)]
    admin_host: IpAddr,

    /// Port for the admin API and web console.
    #[arg(long, default_value_t = DEFAULT_ADMIN_PORT)]
    admin_port: u16,

    /// Bearer token required by the admin API.
    #[arg(long, env = "CONTROL_TOKEN", value_name = "TOKEN")]
    admin_token: String,

    /// TCP listen backlog passed to the data plane.
    #[arg(long, default_value_t = DEFAULT_LISTEN_BACKLOG)]
    backlog: u32,

    /// Maximum time active requests may drain during shutdown.
    #[arg(long, default_value_t = humantime::Duration::from(DEFAULT_GRACEFUL_SHUTDOWN_TIMEOUT))]
    shutdown_timeout: humantime::Duration,

    /// Validate the configuration without binding sockets.
    #[arg(long)]
    dry_run: bool,

    /// Append durable per-request routing records to this JSONL file.
    #[arg(long, value_name = "PATH")]
    routing_log_file: Option<PathBuf>,

    /// TLS certificate path in PEM format for the data plane.
    #[arg(long, requires = "tls_key")]
    tls_cert: Option<PathBuf>,

    /// TLS private-key path in PEM format for the data plane.
    #[arg(long, requires = "tls_cert")]
    tls_key: Option<PathBuf>,
}

impl ControlArgs {
    /// Parses command-line arguments using clap.
    pub fn parse_args() -> Self {
        Self::parse()
    }
}

/// Loads the deployment and serves both planes until a shutdown signal.
pub async fn run(args: ControlArgs) -> Result<(), String> {
    let token = args.admin_token.trim().to_string();
    if token.is_empty() {
        return Err("--admin-token (or CONTROL_TOKEN) must not be empty".to_string());
    }
    let config_path = &args.config;
    let secrets_path = args
        .secrets
        .clone()
        .unwrap_or_else(|| config_path.with_file_name("secrets.toml"));
    let secrets = Arc::new(
        SecretsStore::open(&secrets_path)
            .map_err(|error| format!("open {}: {error}", secrets_path.display()))?,
    );
    secrets.apply_to_env();
    let runner = Runner::load(config_path).map_err(|error| error.to_string())?;
    let mut state = ServerState::from_runner(runner).map_err(|error| error.to_string())?;
    if let Some(path) = args.routing_log_file.clone() {
        state = state
            .with_routing_log(path)
            .map_err(|error| error.to_string())?;
    }
    let store = Arc::new(
        ConfigStore::open(config_path)
            .map_err(|error| format!("open {}: {error}", config_path.display()))?,
    );
    if args.dry_run {
        let models: Vec<String> = state.models();
        println!("config OK; routes: {}", models.join(", "));
        return Ok(());
    }
    let tls = match (args.tls_cert.clone(), args.tls_key.clone()) {
        (Some(cert), Some(key)) => {
            if !cert.exists() || !key.exists() {
                return Err(format!(
                    "invalid --tls-cert {} or --tls-key {}: file does not exist",
                    cert.display(),
                    key.display()
                ));
            }
            Some(TlsOptions { cert, key })
        }
        _ => None,
    };
    let data_options = ServerRunOptions {
        addr: SocketAddr::new(args.host, args.port),
        backlog: args.backlog,
        dry_run: false,
        shutdown_timeout: args.shutdown_timeout.into(),
        tls,
    };
    let data_server =
        BoundServer::bind(state.clone(), data_options).map_err(|error| error.to_string())?;
    let data_addr = data_server.local_addr();
    let admin_state = Arc::new(AdminState::new(
        state,
        store,
        secrets,
        token,
        args.routing_log_file.clone(),
    ));
    let app = Router::new()
        .nest("/admin", admin_router(admin_state.clone()))
        .fallback(web_ui);
    let admin_addr = SocketAddr::new(args.admin_host, args.admin_port);
    let admin_listener = TcpListener::bind(admin_addr)
        .await
        .map_err(|error| format!("bind admin address {admin_addr}: {error}"))?;
    println!("switchyard-control: data plane on http://{data_addr}");
    println!(
        "switchyard-control: admin API and web console on http://{admin_addr} (bearer token required)"
    );
    let data_task = tokio::spawn(async move {
        data_server
            .serve(shutdown_signal())
            .await
            .map_err(|error| error.to_string())
    });
    let admin_task = tokio::spawn(async move {
        axum::serve(admin_listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await
            .map_err(|error| error.to_string())
    });
    data_task
        .await
        .map_err(|error| format!("data plane task: {error}"))
        .and_then(|result| result)?;
    admin_task
        .await
        .map_err(|error| format!("admin task: {error}"))
        .and_then(|result| result)?;
    Ok(())
}

/// Resolves on SIGINT or SIGTERM.
async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    let terminate = async {
        #[cfg(unix)]
        {
            let mut signal =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("SIGTERM handler");
            signal.recv().await
        }
        #[cfg(not(unix))]
        {
            std::future::pending().await
        }
    };
    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}
