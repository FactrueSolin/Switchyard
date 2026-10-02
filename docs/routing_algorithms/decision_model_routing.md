# Decision-Model Routing

**Decision-model** routing uses a dedicated decision model to pick the
efficient model or the capable model for each request. The decision model does
not generate text. One forward pass returns the chosen option and a full
probability distribution, so the call stays fast even for a long transcript.

Configure it with `type = "decision_model"`.

> Requires an unreleased feature. [Build from source](../getting_started.md#build-from-source) to run this example.

## Configure a decision-model route

The decision model client does not use `[llm_clients]`, because the provider
endpoint is not a chat-completions endpoint. Configure it either inline in the
route table, or as a named `[decision_models.<name>]` entry the route
references with `decision = "<name>"`. The two forms are mutually exclusive.

```toml
schema_version = 1

[llm_clients.openrouter]
format = "openai_chat"
base_url = "https://openrouter.ai/api/v1"
api_key_env = "OPENROUTER_API_KEY"

[targets.strong]
id = "openai/gpt-4o"
llm_client = "openrouter"

[targets.weak]
id = "z-ai/glm-5.2"
llm_client = "openrouter"

[routes.smart]
id = "smart"
type = "decision_model"
strong_target = "strong"
weak_target = "weak"
default_target = "weak"
decision_base_url = "https://api.typesafe.ai/v1"
decision_api_key_env = "TYPESAFE_API_KEY"
decision_model = "jev-latest"
classify_trigger = "every_request"
```

`decision_base_url` is the provider's OpenAI-compatible `v1` root. The runner
appends `/systemone` to it. Use the root for whatever System One provider you
use — `https://api.typesafe.ai/v1`, a DashScope workspace
`https://{workspace-id}.{region}.maas.aliyuncs.com/compatible-mode/v1`, or a
LiteLLM proxy `{base}/typesafe`. `decision_model` is the model name the
provider lists at `/v1/models`.

The named form keeps one endpoint definition shared by several routes:

```toml
[decision_models.typesafe]
base_url = "https://api.typesafe.ai/v1"
model = "jev-latest"
api_key_env = "TYPESAFE_API_KEY"

[routes.smart]
id = "smart"
type = "decision_model"
strong_target = "strong"
weak_target = "weak"
default_target = "weak"
decision = "typesafe"
```

The target table names are local references. Their `id` values are the model
identifiers sent to the upstream provider. The route's `id`, `smart`, is the
model name clients send to Switchyard.

## How the decision works

On every trigger the router sends a compressed transcript of the conversation
to the decision model:

- the system prompt, truncated at 300 characters. Long agent system prompts
  are mostly tool documentation, and they dilute the difficulty signal: the
  decision model scores hard tasks lower the longer the prompt is
- the trailing user turns, default 2, set with `recent_turn_window`
- tool call names, such as `[tool call: run_build]`, and the opening 200
  characters of each tool result: test output, errors, and file snippets are
  strong difficulty signals

Media and reasoning blocks are dropped. Each message is truncated at 2,000
characters; the latest user turn, the request being routed, is truncated at
8,000.

This is truncation and filtering, not summarization: no model call, fully
deterministic, a few hundred to a few thousand characters from any
transcript. A summarizing step would add its own latency and cost to every
routing decision, which defeats the point of a 50 ms decision call.

The decision model answers a fixed choice question with the options `strong`,
`weak`, and `other`, and returns the probability of every option. The router
routes on the distribution, not on the API's reported confidence: the
confidence does not track it (a correct strong pick can report 0.44 while
P(strong) is 0.68), and `other` carries a 0.2-0.4 baseline in agent
transcripts that would veto clear picks.

Two modes pick the tier:

**Trust the model (default, `confidence_threshold` unset).** The option the
model picks routes directly. `strong` goes to `strong_target`, `weak` to
`weak_target`, and `other` to `default_target`. The pick is the argmax of the
distribution, so it never flips between two requests over a small score
change.

**Threshold mode (`confidence_threshold` set).** Route on the distribution:

| Condition | Route |
|---|---|
| `P(strong)` at or above the threshold | `strong_target` |
| `P(weak)` at or above the threshold (and `P(strong)` below it) | `weak_target` |
| Neither above the threshold | `default_target` |

The threshold sits where you want the strong/weak boundary. Set it high (0.7)
to keep the strong tier rare; set it low (0.5) to catch hard tasks the model
scores hesitantly. A threshold near the `other` baseline (0.2-0.4) can flip a
session between tiers on a 0.04 score wobble; the trust mode above avoids that.

A failed call logs a warning and falls through to `default_target` in both
modes. The request is not stopped.

## Example decision round-trip

The `state` field is one plain-text string built from the conversation. A real
call for a Claude Code session looks like this:

```text
system: You are Claude Code, an interactive agent that helps users with software
engineering tasks. Use the tools below to edit files, run commands, and search
the codebase.

# Tools

## Bash
Execute shell commands in the user's workspace. ...

#…[truncated]                        <- system prompt cut at 300 characters
user: Implement a parallel Langevin dynamics simulator in Rust: satisfy the
fluctuation-dissipation theorem, explain the Euler error term; use a fixed
cell list with rayon for the repulsion; validate MSD against theory.
                                         <- request being routed, up to 8,000 chars
assistant: I'll start by checking the existing test layout. [tool call: Bash]
tool: [tool result: cargo test: 3 passed; 1 failed: langevin::msd_converges --
assertion failed at line 214: msd error 0.31 > 0.1; …[truncated]]
                                         <- tool result, first 200 characters
assistant: [tool call: Read]
tool: [tool result: fn simulate(n: u64, steps: u64) -> Vec<Particle> { …[truncated]]
```

Kept: role prefixes, the system prompt (300 characters or fewer), user turns
(8,000 for the latest), assistant text (2,000), tool call names, and the opening
200 characters of each tool result. Dropped: the rest of each tool result,
media (an `[image]` placeholder stands in), reasoning blocks, and tool call
argument JSON.

The full request is one JSON POST to `{decision_base_url}/systemone`:

```json
{
  "model": "jev-latest",
  "state": "<the transcript above, one string>",
  "questions": {
    "tier": {
      "type": "choice",
      "instructions": "Which model should serve the latest user request?",
      "criteria": {
        "strong": "Needs deep multi-step reasoning, precise long-range dependencies, or the session is mid-way through a hard problem (debugging, architecture, subtle correctness)",
        "weak": "Routine and low-risk: lookups, chat, simple edits, boilerplate, mechanical transformations",
        "other": "Mixed signals; neither clearly applies"
      }
    }
  }
}
```

The field names `model`, `state`, `tier`, `type`, `instructions`, and
`criteria` are fixed by the System One wire protocol shared by every provider.
Only their values are free text. The values can be in any language the model
reads, so the `criteria` text can be written in whatever language the
decision model you chose handles best. The option names `strong`, `weak`, and
`other` are the routing contract: the router maps them to `strong_target`,
`weak_target`, and the default target. Renaming an option breaks the mapping
unless the code is changed to match.

A real response:

```json
{
  "model": "jev-1.13.0",
  "request_id": "6013f066-ae1a-94c2-bb3f-ccb7c80eeb94",
  "answers": {
    "tier": {
      "type": "choice",
      "choice": "other",
      "confidence": 0.41,
      "probabilities": { "strong": 0.31, "weak": 0.09, "other": 0.61 }
    }
  },
  "usage": { "input_tokens": 412 },
  "latency_ms": 53.6
}
```

- `choice` — the option the model selected.
- `probabilities` — the full distribution over all options, summing to 1.
  The router decides from this field.
- `confidence` — the API's self-reported confidence. It does not track the
  distribution, so the router logs it and does not route on it.
- `usage` and `latency_ms` — token count and provider-side time; about 50 ms
  for a 400-token transcript.

## When the decision model runs

`classify_trigger` follows the same values as the
[LLM classifier](llm_classifier_routing.md):

- `every_request` (default) decides every request, tool continuations included.
- `user_turn` decides each new user message and keeps that target across the
  tool calls between. Needs a session ID, or `message_hash_fallback = true`.
- `new_session` decides once and reuses that target for the session. Needs a
  session ID, or `message_hash_fallback = true`.

With `user_turn` or `new_session`, a retained target short-circuits the call:
requests that reuse the target never reach the decision model.

## Related documentation

- [TOML schema](../reference/toml_schema.md#decision_model)
- [LLM Classifier Routing](llm_classifier_routing.md)
- [Core Concepts](../core_concepts.md)
