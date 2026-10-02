# switchyard-control

Control plane for Switchyard decision-model routing deployments. One process
runs both planes: the unmodified `switchyard-server` data plane and a
bearer-token admin API with an embedded web console.

The deployment TOML file stays the single source of truth. Every console
mutation edits that file, validates the result with `Runner::from_toml`,
persists it atomically, and hot-swaps the route table — no restart.

## Run

```sh
switchyard-control \
  --config /etc/switchyard/deployment.toml \
  --admin-token "$CONTROL_TOKEN" \
  --routing-log-file /var/log/switchyard/routing.jsonl
```

- Data plane: `http://0.0.0.0:4000` (`--host`, `--port`)
- Admin API + web console: `http://0.0.0.0:4001` (`--admin-host`, `--admin-port`)
- Secrets: `secrets.toml` beside the config file by default (`--secrets`),
  loaded into the process environment before the deployment file is read.

The console manages `[decision_models.<name>]` entries (base URL, model name,
API key env, one-click connection test), routes, LLM clients, and targets in
the deployment's existing TOML format, plus secrets and configuration history
with restore.