// Switchyard control console.

// ---- i18n ---------------------------------------------------------------

const I18N = {
  en: {
    brand: "Switchyard Control",
    token_placeholder: "admin token",
    connect: "Connect",
    tab_overview: "Overview",
    tab_models: "Decision Models",
    tab_strategies: "Strategies",
    tab_secrets: "Secrets",
    tab_history: "History",
    routes: "Routes",
    recent_decisions: "Recent decisions",
    decisions_model_ph: "route id (blank = all)",
    load: "Load",
    raw_config: "Raw configuration",
    apply_config: "Apply configuration",
    load_template: "Load template",
    template_loaded: "template loaded — edit the values, then Apply",
    add_or_update: "Add or update",
    name: "name",
    base_url_ph: "https://api.typesafe.ai/v1",
    model_ph: "model name",
    key_env_ph: "API key env (blank = NAME_API_KEY)",
    save: "Save",
    strategies_intro: "Entries use the deployment TOML format as-is. Editing an entry loads its block into the form below; saving validates the whole file and hot-swaps the route table.",
    add_entry: "Add entry",
    entry_name_ph: "entry name",
    section_block_ph: "table body in TOML, e.g. id = \"...\"",
    env_name_ph: "env name",
    value_ph: "value",
    set: "Set",
    config_history: "Configuration history",
    h_route: "route", h_id: "id", h_type: "type", h_decision: "decision",
    h_time: "time", h_model: "model", h_tier: "tier", h_choice: "choice", h_probs: "probabilities",
    h_name: "name", h_used_by: "used by", h_client: "client", h_target: "target", h_value: "value",
    h_version: "version (replaced by next apply)", h_size: "size",
    no_routes: "no routes configured",
    no_decisions: "no routing decisions recorded yet (routing log disabled or empty)",
    no_models: "no decision models configured",
    none_configured: "none configured",
    no_secrets: "no secrets stored",
    no_changes: "no changes recorded yet",
    unused: "unused",
    sec_routes: "Routes", sec_clients: "LLM clients", sec_targets: "Targets",
    act_test: "Test", act_edit: "Edit", act_delete: "Delete", act_remove: "Remove",
    act_view: "View", act_restore: "Restore",
    applying: "applying…",
    applied_version: (v) => `applied, version ${v}`,
    testing: (n) => `testing ${n}…`,
    saved: "saved",
    saving: "saving…",
    edit_entry: (s, n) => `Edit ${s}.${n}`,
    applied_entry: (s, n) => `applied ${s}.${n}`,
    confirm_delete_model: (n) => `Delete decision model ${n}?`,
    confirm_delete_entry: (s, n) => `Delete ${s}.${n}?`,
    confirm_remove_secret: (n) => `Remove secret ${n}?`,
    confirm_restore: (v) => `Restore version ${v} as a new version?`,
    auth_enter_token: "enter a token first",
    auth_connected: (v) => `connected, config v${v}`,
    auth_unauthorized: "unauthorized — token cleared, enter the correct one",
    decision_model_ref: (n) => `decision model: ${n}`,
    inline_ref: (m, u) => `inline: ${m} @ ${u}`,
    model_ok: (n, o) => `${n}: ok — choice ${o.choice}, P(${o.choice}) = ${o.probabilities[o.choice]}`,
    model_fail: (n, e) => `${n}: ${e}`,
    secret_saved_reload: (v) => `saved, deployment reloaded (version ${v})`,
    secret_saved_fail: (e) => `saved, but reload failed: ${e}`,
    restored: (v) => `restored as version ${v}`,
    bytes: (n) => `${n} bytes`,
  },
  zh: {
    brand: "Switchyard 控制台",
    token_placeholder: "管理令牌",
    connect: "连接",
    tab_overview: "概览",
    tab_models: "决策模型",
    tab_strategies: "策略",
    tab_secrets: "密钥",
    tab_history: "历史",
    routes: "路由",
    recent_decisions: "最近决策",
    decisions_model_ph: "路由 id（留空 = 全部）",
    load: "加载",
    raw_config: "原始配置",
    apply_config: "应用配置",
    load_template: "载入模板",
    template_loaded: "已载入模板 — 修改取值后点应用",
    add_or_update: "添加或更新",
    name: "名称",
    base_url_ph: "https://api.typesafe.ai/v1",
    model_ph: "模型名",
    key_env_ph: "API 密钥环境变量（留空 = NAME_API_KEY）",
    save: "保存",
    strategies_intro: "条目使用部署 TOML 的原生格式。编辑会把对应区块载入下方表单；保存时校验整个文件并热替换路由表。",
    add_entry: "新增条目",
    entry_name_ph: "条目名称",
    section_block_ph: "TOML 表体，例如 id = \"...\"",
    env_name_ph: "环境变量名",
    value_ph: "值",
    set: "设置",
    config_history: "配置历史",
    h_route: "路由", h_id: "id", h_type: "类型", h_decision: "决策",
    h_time: "时间", h_model: "模型", h_tier: "档位", h_choice: "选择", h_probs: "概率",
    h_name: "名称", h_used_by: "被引用", h_client: "客户端", h_target: "目标", h_value: "值",
    h_version: "版本（下次应用时替换）", h_size: "大小",
    no_routes: "未配置路由",
    no_decisions: "暂无路由决策记录（路由日志未开启或为空）",
    no_models: "未配置决策模型",
    none_configured: "未配置",
    no_secrets: "未存储密钥",
    no_changes: "暂无变更记录",
    unused: "未使用",
    sec_routes: "路由", sec_clients: "LLM 客户端", sec_targets: "目标",
    act_test: "测试", act_edit: "编辑", act_delete: "删除", act_remove: "移除",
    act_view: "查看", act_restore: "恢复",
    applying: "应用中…",
    applied_version: (v) => `已应用，版本 ${v}`,
    testing: (n) => `正在测试 ${n}…`,
    saved: "已保存",
    saving: "保存中…",
    edit_entry: (s, n) => `编辑 ${s}.${n}`,
    applied_entry: (s, n) => `已应用 ${s}.${n}`,
    confirm_delete_model: (n) => `删除决策模型 ${n}？`,
    confirm_delete_entry: (s, n) => `删除 ${s}.${n}？`,
    confirm_remove_secret: (n) => `移除密钥 ${n}？`,
    confirm_restore: (v) => `将版本 ${v} 恢复为新版本？`,
    auth_enter_token: "请先输入令牌",
    auth_connected: (v) => `已连接，配置版本 v${v}`,
    auth_unauthorized: "令牌无效 — 已清除，请重新输入",
    decision_model_ref: (n) => `决策模型：${n}`,
    inline_ref: (m, u) => `内联：${m} @ ${u}`,
    model_ok: (n, o) => `${n}：正常 — 选择 ${o.choice}，P(${o.choice}) = ${o.probabilities[o.choice]}`,
    model_fail: (n, e) => `${n}：${e}`,
    secret_saved_reload: (v) => `已保存，部署已重载（版本 ${v}）`,
    secret_saved_fail: (e) => `已保存，但重载失败：${e}`,
    restored: (v) => `已恢复为版本 ${v}`,
    bytes: (n) => `${n} 字节`,
  },
};

let lang =
  localStorage.getItem("sy_lang") ||
  (navigator.language && navigator.language.toLowerCase().startsWith("zh") ? "zh" : "en");

function t(key, ...args) {
  const entry = (I18N[lang] && I18N[lang][key]) || I18N.en[key] || key;
  return typeof entry === "function" ? entry(...args) : entry;
}

// Fills every [data-i18n] label and [data-i18n-ph] placeholder from the dict.
function applyStaticI18n() {
  document.querySelectorAll("[data-i18n]").forEach((node) => {
    node.textContent = t(node.getAttribute("data-i18n"));
  });
  document.querySelectorAll("[data-i18n-ph]").forEach((node) => {
    node.placeholder = t(node.getAttribute("data-i18n-ph"));
  });
  document.title = t("brand");
  document.documentElement.lang = lang;
  // The toggle shows the language it would switch TO.
  $("lang-toggle").textContent = lang === "zh" ? "EN" : "中文";
}

// ---- State and helpers ---------------------------------------------------

const state = {
  token: localStorage.getItem("sy_token") || "",
  editing: null, // { section, name } while the form edits an existing entry
};

const $ = (id) => document.getElementById(id);

function api(path, options = {}) {
  const headers = { Authorization: `Bearer ${state.token}` };
  if (options.body !== undefined) headers["Content-Type"] = "application/json";
  return fetch(`/admin${path}`, {
    ...options,
    headers,
    body: options.body !== undefined ? JSON.stringify(options.body) : undefined,
  }).then(async (res) => {
    const text = await res.text();
    let data;
    try { data = JSON.parse(text); } catch { data = text; }
    if (!res.ok) {
      const message = data && data.error ? data.error : `HTTP ${res.status}`;
      throw new Error(message);
    }
    return data;
  });
}

function status(el, message, isError) {
  el.textContent = message || "";
  el.classList.toggle("error", Boolean(isError));
}

function el(tag, text, className) {
  const node = document.createElement(tag);
  if (text !== undefined) node.textContent = text;
  if (className) node.className = className;
  return node;
}

function button(label, onClick, className) {
  const node = el("button", label, className || "");
  node.addEventListener("click", onClick);
  return node;
}

function table(headers, rows) {
  const wrap = el("div", null, "table-wrap");
  const table = el("table");
  const head = el("tr");
  headers.forEach((h) => head.appendChild(el("th", h)));
  table.appendChild(head);
  rows.forEach((cells) => {
    const row = el("tr");
    cells.forEach((cell) => {
      const td = el("td");
      if (cell instanceof Node) td.appendChild(cell);
      else td.textContent = cell === null || cell === undefined ? "" : String(cell);
      row.appendChild(td);
    });
    table.appendChild(row);
  });
  wrap.appendChild(table);
  return wrap;
}

// ---- Overview ----------------------------------------------------------

async function loadOverview() {
  const summary = await api("/summary");
  const routes = Object.entries(summary.routes || {}).map(([name, route]) => {
    let decision = "";
    if (route.decision) {
      decision = route.decision.name
        ? t("decision_model_ref", route.decision.name)
        : t("inline_ref", route.decision.model || "?", route.decision.base_url || "?");
    }
    return [name, route.id || "", route.type || "", decision];
  });
  $("routes-view").replaceChildren(
    routes.length
      ? table([t("h_route"), t("h_id"), t("h_type"), t("h_decision")], routes)
      : el("p", t("no_routes"))
  );
  const config = await api("/config");
  $("config-editor").value = config.source;
}

$("save-config").addEventListener("click", async () => {
  status($("config-status"), t("applying"));
  try {
    const report = await api("/config", {
      method: "PUT",
      body: { source: $("config-editor").value },
    });
    status($("config-status"), t("applied_version", report.version));
    loadOverview();
  } catch (error) {
    status($("config-status"), error.message, true);
  }
});

$("load-template").addEventListener("click", async () => {
  status($("config-status"), "");
  try {
    const res = await fetch("/deployment.template.toml");
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    $("config-editor").value = await res.text();
    status($("config-status"), t("template_loaded"));
  } catch (error) {
    status($("config-status"), error.message, true);
  }
});

$("decisions-load").addEventListener("click", async () => {
  const model = $("decisions-model").value.trim();
  const query = model ? `?model=${encodeURIComponent(model)}` : "";
  try {
    const data = await api(`/decisions${query}`);
    const rows = (data.decisions || []).map((d) => [
      d.ts || "",
      d.route_id || "",
      d.model || "",
      d.tier || "",
      d.choice || "",
      d.probabilities ? JSON.stringify(d.probabilities) : "",
    ]);
    $("decisions-view").replaceChildren(
      rows.length
        ? table([t("h_time"), t("h_route"), t("h_model"), t("h_tier"), t("h_choice"), t("h_probs")], rows)
        : el("p", t("no_decisions"))
    );
  } catch (error) {
    $("decisions-view").replaceChildren(el("p", error.message));
  }
});

// ---- Decision models ----------------------------------------------------

async function loadModels() {
  const summary = await api("/summary");
  const entries = Object.entries(summary.decision_models || {});
  if (!entries.length) {
    $("models-view").replaceChildren(el("p", t("no_models")));
    return;
  }
  const rows = entries.map(([name, model]) => {
    const actions = el("div", null, "row-actions");
    actions.appendChild(button(t("act_test"), async () => {
      status($("models-status"), t("testing", name));
      try {
        const result = await api(`/decision-models/${encodeURIComponent(name)}/test`, {
          method: "POST",
          body: {},
        });
        status(
          $("models-status"),
          result.ok
            ? t("model_ok", name, result.outcome)
            : t("model_fail", name, result.error),
          !result.ok
        );
      } catch (error) {
        status($("models-status"), t("model_fail", name, error.message), true);
      }
    }));
    actions.appendChild(button(t("act_edit"), () => {
      $("model-name").value = name;
      $("model-base-url").value = model.base_url || "";
      $("model-model").value = model.model || "";
      $("model-key-env").value = model.api_key_env || "";
    }));
    actions.appendChild(button(t("act_delete"), async () => {
      if (!confirm(t("confirm_delete_model", name))) return;
      try {
        await api(`/decision-models/${encodeURIComponent(name)}`, { method: "DELETE" });
        loadModels();
      } catch (error) {
        status($("models-status"), error.message, true);
      }
    }));
    return [
      name,
      model.base_url || "",
      model.model || "",
      model.api_key_env || "",
      (model.routes || []).join(", ") || t("unused"),
      actions,
    ];
  });
  $("models-view").replaceChildren(
    table([t("h_name"), "base_url", "model", "api_key_env", t("h_used_by"), ""], rows)
  );
}

$("model-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const name = $("model-name").value.trim();
  const body = {
    base_url: $("model-base-url").value.trim(),
    model: $("model-model").value.trim(),
  };
  if ($("model-key-env").value.trim()) body.api_key_env = $("model-key-env").value.trim();
  status($("models-status"), t("saving"));
  try {
    const path = name ? `/decision-models/${encodeURIComponent(name)}` : "/decision-models";
    if (name) {
      await api(path, { method: "PUT", body });
    } else {
      await api(path, { method: "POST", body: { ...body, name } });
    }
    status($("models-status"), t("saved"));
    $("model-form").reset();
    loadModels();
  } catch (error) {
    status($("models-status"), error.message, true);
  }
});

// ---- Strategies ----------------------------------------------------------

async function loadSections() {
  const view = $("sections-view");
  view.replaceChildren();
  const sectionLabels = {
    routes: t("sec_routes"),
    llm_clients: t("sec_clients"),
    targets: t("sec_targets"),
  };
  for (const section of ["routes", "llm_clients", "targets"]) {
    view.appendChild(el("h3", sectionLabels[section]));
    try {
      const data = await api(`/sections/${section}`);
      const names = data.entries || [];
      if (!names.length) {
        view.appendChild(el("p", t("none_configured")));
        continue;
      }
      const rows = names.map((name) => {
        const actions = el("div", null, "row-actions");
        actions.appendChild(button(t("act_edit"), async () => {
          try {
            const entry = await api(`/sections/${section}/${encodeURIComponent(name)}`);
            $("section-select").value = section;
            $("section-name").value = name;
            $("section-block").value = entry.block;
            $("section-edit-title").textContent = t("edit_entry", section, name);
          } catch (error) {
            status($("sections-status"), error.message, true);
          }
        }));
        actions.appendChild(button(t("act_delete"), async () => {
          if (!confirm(t("confirm_delete_entry", section, name))) return;
          try {
            await api(`/sections/${section}/${encodeURIComponent(name)}`, { method: "DELETE" });
            loadSections();
          } catch (error) {
            status($("sections-status"), error.message, true);
          }
        }));
        return [name, actions];
      });
      view.appendChild(table([sectionHeaderName(section), ""], rows));
    } catch (error) {
      view.appendChild(el("p", error.message));
    }
  }
}

function sectionHeaderName(section) {
  return section === "llm_clients" ? t("h_client") : section === "targets" ? t("h_target") : t("h_route");
}

$("section-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const section = $("section-select").value;
  const name = $("section-name").value.trim();
  const body = { block: $("section-block").value };
  status($("sections-status"), t("applying"));
  try {
    const path = name ? `/sections/${section}/${encodeURIComponent(name)}` : `/sections/${section}`;
    if (name) {
      await api(path, { method: "PUT", body });
    } else {
      await api(path, { method: "POST", body: { ...body, name } });
    }
    status($("sections-status"), t("applied_entry", section, name));
    $("section-block").value = "";
    $("section-name").value = "";
    $("section-edit-title").textContent = t("add_entry");
    loadSections();
  } catch (error) {
    status($("sections-status"), error.message, true);
  }
});

// ---- Secrets -------------------------------------------------------------

async function loadSecrets() {
  const data = await api("/secrets");
  const names = data.names || [];
  if (!names.length) {
    $("secrets-view").replaceChildren(el("p", t("no_secrets")));
    return;
  }
  const rows = names.map((name) => {
    const actions = el("div", null, "row-actions");
    actions.appendChild(button(t("act_remove"), async () => {
      if (!confirm(t("confirm_remove_secret", name))) return;
      try {
        await api(`/secrets/${encodeURIComponent(name)}`, { method: "DELETE" });
        loadSecrets();
      } catch (error) {
        status($("secrets-status"), error.message, true);
      }
    }));
    return [name, "••••••••", actions];
  });
  $("secrets-view").replaceChildren(table([t("h_name"), t("h_value"), ""], rows));
}

$("secret-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  status($("secrets-status"), t("saving"));
  try {
    const result = await api("/secrets", {
      method: "PUT",
      body: { name: $("secret-name").value.trim(), value: $("secret-value").value },
    });
    status(
      $("secrets-status"),
      result.reloaded
        ? t("secret_saved_reload", result.version)
        : t("secret_saved_fail", result.error),
      !result.reloaded
    );
    $("secret-form").reset();
    loadSecrets();
  } catch (error) {
    status($("secrets-status"), error.message, true);
  }
});

// ---- History -------------------------------------------------------------

async function loadHistory() {
  const data = await api("/history");
  const entries = data.entries || [];
  if (!entries.length) {
    $("history-view").replaceChildren(el("p", t("no_changes")));
    return;
  }
  const rows = entries.map((entry) => {
    const actions = el("div", null, "row-actions");
    actions.appendChild(button(t("act_view"), async () => {
      try {
        const source = await api(`/history/${entry.version}`);
        const pre = $("history-source");
        pre.textContent = source.source;
        pre.classList.remove("hidden");
      } catch (error) {
        status($("history-status"), error.message, true);
      }
    }));
    actions.appendChild(button(t("act_restore"), async () => {
      if (!confirm(t("confirm_restore", entry.version))) return;
      try {
        const report = await api(`/history/${entry.version}/restore`, { method: "POST" });
        status($("history-status"), t("restored", report.version));
        loadHistory();
      } catch (error) {
        status($("history-status"), error.message, true);
      }
    }));
    return [entry.version, t("bytes", entry.bytes), actions];
  });
  $("history-view").replaceChildren(
    table([t("h_version"), t("h_size"), ""], rows)
  );
}

// ---- Tabs and auth ---------------------------------------------------------

const LOADERS = {
  overview: () => loadOverview(),
  models: () => loadModels(),
  strategies: () => loadSections(),
  secrets: () => loadSecrets(),
  history: () => loadHistory(),
};

let activeTab = "overview";

function loadActiveTab() {
  LOADERS[activeTab]()
    .catch((error) => {
      if (activeTab === "overview") $("config-status").textContent = error.message;
    });
}

function connect() {
  // Prefer what the user typed; otherwise keep the token loaded from
  // localStorage on refresh. The input is cleared after connecting.
  const entered = $("token").value.trim();
  if (entered) {
    state.token = entered;
    localStorage.setItem("sy_token", state.token);
  }
  $("token").value = "";
  if (!state.token) {
    status($("auth-status"), t("auth_enter_token"), true);
    return;
  }
  api("/health")
    .then((health) => {
      status($("auth-status"), t("auth_connected", health.version));
      loadActiveTab();
    })
    .catch((error) => {
      // A stored token the server rejects is worse than none: drop it so the
      // user is prompted again instead of silently failing on every refresh.
      if (error.message === "unauthorized") {
        localStorage.removeItem("sy_token");
        state.token = "";
        status($("auth-status"), t("auth_unauthorized"), true);
      } else {
        status($("auth-status"), error.message, true);
      }
    });
}

$("connect").addEventListener("click", connect);
$("token").addEventListener("keydown", (event) => {
  if (event.key === "Enter") {
    event.preventDefault();
    connect();
  }
});

$("lang-toggle").addEventListener("click", () => {
  lang = lang === "zh" ? "en" : "zh";
  localStorage.setItem("sy_lang", lang);
  applyStaticI18n();
  if (state.token) loadActiveTab();
});

document.querySelectorAll("#tabs button").forEach((tabBtn) => {
  tabBtn.addEventListener("click", () => {
    document.querySelectorAll("#tabs button").forEach((b) => b.classList.remove("active"));
    document.querySelectorAll(".tab").forEach((sec) => sec.classList.add("hidden"));
    tabBtn.classList.add("active");
    $(`tab-${tabBtn.dataset.tab}`).classList.remove("hidden");
    activeTab = tabBtn.dataset.tab;
    if (state.token) loadActiveTab();
  });
});

// Bootstrap: paint the UI in the saved language, then auto-connect with the
// persisted token (if any) so a refresh never asks for it again.
applyStaticI18n();
if (state.token) connect();