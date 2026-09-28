# 决策模型路由

**决策模型**路由用一个专门的决策模型为每个请求选择性价比模型（weak）或高智能模型（strong）。决策模型不生成文本：一次前向返回选中选项和完整的概率分布，所以即使会话很长，这次调用也很快。

配置方式：`type = "decision_model"`。

> 依赖一个尚未发布的功能。[从源码构建](../getting_started.md#build-from-source)才能运行这个例子。

## 配置决策模型路由

决策模型的客户端直接写在路由表里，不使用 `[llm_clients]`，因为它的端点不是 chat-completions 端点。

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
decision_base_url = "https://{workspace-id}.cn-beijing.maas.aliyuncs.com/compatible-mode/v1"
decision_api_key_env = "DASHSCOPE_API_KEY"
classify_trigger = "every_request"
```

`decision_base_url` 是服务商的 OpenAI 兼容根地址。Runner 会在它后面拼上
`/systemone`。把 `{workspace-id}` 换成你的工作空间 ID。

目标表的名字是本地引用。`id` 的值才是发给上游服务商的模型标识。路由的
`id`（这里是 `smart`）是客户端发给 Switchyard 的模型名。

## 决策过程

每次触发时，路由器把会话的压缩文本发给决策模型：

- 系统提示，截断到 300 字符。Agent 的长系统提示大多在描述工具，会稀释难度
  信号：提示越长，决策模型给难题打的强档概率越低
- 末尾的用户轮次，默认 2 轮，用 `recent_turn_window` 设置
- 工具调用名（如 `[tool call: run_build]`）和每个工具结果的前 200 字符：
  测试输出、报错、文件片段都是强难度信号

媒体块和推理块会被丢弃。每条消息截断到 2,000 字符；最新的一条用户轮次
（也就是正在路由的请求）截断到 8,000。

这是截断和过滤，不是摘要：不调用任何模型，完全确定性，任何会话都能压到
几百到几千字符。加一步摘要会给每次路由决策再叠一层延迟和成本，违背了
决策调用只要 50ms 的初衷。

决策模型回答一个固定的选择题，选项为 `strong`、`weak`、`other`，并返回每个
选项的概率。路由器不读 API 自报的 confidence：实测 confidence 与概率分布
不同步（判对 strong 时可能报 0.44，而 P(strong) 是 0.68）。

选档有两种模式：

**信任模型（默认，不写 `confidence_threshold`）。** 模型选哪个就路由到哪个：
`strong` 走 `strong_target`，`weak` 走 `weak_target`，`other` 走
`default_target`。模型的选中项就是概率分布的 argmax，不会因为分数小幅波动
在两次请求之间翻转档位。

**阈值模式（设置 `confidence_threshold`）。** 按概率分布决定：

| 条件 | 路由结果 |
|---|---|
| `P(strong)` 达到阈值 | `strong_target` |
| `P(weak)` 达到阈值（且 `P(strong)` 未达） | `weak_target` |
| 两者都未达 | `default_target` |

阈值决定强弱档的分界线在哪。调高（0.7）让强模型更省；调低（0.5）接住模型
判得犹豫的难题。阈值卡在 `other` 的基线区间（0.2-0.4）附近时，0.04 的分数
波动就能让会话在两个档之间来回跳；信任模式没有这个问题。

两种模式下，调用失败都会记录一条警告并落到 `default_target`，请求不会中断。

## 决策往返示例

`state` 字段是路由器从会话构造出来的一段纯文本。一个 Claude Code 会话的
真实调用长这样：

```text
system: You are Claude Code, an interactive agent that helps users with software
engineering tasks. Use the tools below to edit files, run commands, and search
the codebase.

# Tools

## Bash
Execute shell commands in the user's workspace. ...

#…[truncated]                        <- 系统提示截到 300 字符
user: Implement a parallel Langevin dynamics simulator in Rust: satisfy the
fluctuation-dissipation theorem, explain the Euler error term; use a fixed
cell list with rayon for the repulsion; validate MSD against theory.
                                         <- 正在路由的请求，上限 8,000 字符
assistant: I'll start by checking the existing test layout. [tool call: Bash]
tool: [tool result: cargo test: 3 passed; 1 failed: langevin::msd_converges --
assertion failed at line 214: msd error 0.31 > 0.1; …[truncated]]
                                         <- 工具结果，只保留前 200 字符
assistant: [tool call: Read]
tool: [tool result: fn simulate(n: u64, steps: u64) -> Vec<Particle> { …[truncated]]
```

**保留的**：角色前缀、系统提示（≤300 字符）、用户轮次（最新一条 ≤8,000）、
assistant 文本（≤2,000）、工具调用名、每个工具结果的前 200 字符。
**丢弃的**：工具结果的其余部分、图片/音视频/文件（用 `[image]` 占位）、
reasoning 块、工具调用的参数 JSON。

完整请求是一次 JSON POST，发往 `{decision_base_url}/systemone`：

```json
{
  "model": "decision-model-preview",
  "state": "<上面那段文本，原样一个字符串>",
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

字段名 `model`、`state`、`tier`、`type`、`instructions`、`criteria` 是服务商
协议固定的，不能改；只有值可以自由写。值可以是模型能读的任何语言：当前的
决策模型是 Qwen 系，中英文都能理解，所以 `criteria` 的措辞可以换成中文。
选项名 `strong`、`weak`、`other` 是路由约定：路由器把它们映射到
`strong_target`、`weak_target` 和默认目标。改选项名会让路由认不出来，除非
同步修改代码里的常量。

真实的返回：

```json
{
  "model": "decision-model-preview",
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

- `choice` — 模型选中的选项。
- `probabilities` — 所有选项的完整概率分布，和为 1。路由器按这个字段做决定。
- `confidence` — API 自报的置信度。它与概率分布不同步，路由器只把它写进日志，
  不用它路由。
- `usage` 和 `latency_ms` — token 数和服务端耗时；400 token 的会话约 50 ms。

## 决策模型何时运行

`classify_trigger` 取值与 [LLM 分类器](llm_classifier_routing.md)相同：

- `every_request`（默认）：每个请求都决策，包括工具续轮。
- `user_turn`：每条新的用户消息决策一次，工具调用之间沿用该目标。需要会话
  ID，或 `message_hash_fallback = true`。
- `new_session`：只在会话开始决策一次，整个会话沿用该目标。需要会话 ID，或
  `message_hash_fallback = true`。

用 `user_turn` 或 `new_session` 时，沿用已选目标会短路调用：复用目标的请求
不会到达决策模型。

## 相关文档

- [TOML schema](../reference/toml_schema.md#decision_model)
- [LLM Classifier Routing](llm_classifier_routing.md)
- [Core Concepts](../core_concepts.md)
