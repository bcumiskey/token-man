const { invoke } = window.__TAURI__.core;
const { getCurrentWindow } = window.__TAURI__.window;

let config;

async function load() {
  config = await invoke("cmd_get_config");
  for (const k of [
    "render_interval_ms",
    "admin_poll_interval_s",
    "otel_port",
  ]) {
    const el = document.getElementById(k);
    if (el) el.value = config.app[k];
  }
  for (const k of [
    "minimize_to_tray",
    "autostart",
    "always_on_top_default",
    "jingle_on_launch",
    "notification_sound",
    "notification_toasts",
    "red_mode_enabled",
  ]) {
    const el = document.getElementById(k);
    if (el) el.checked = config.app[k];
  }
  for (const k of Object.keys(config.thresholds)) {
    const el = document.getElementById(k);
    if (el) el.value = config.thresholds[k];
  }
  document.getElementById("admin_api_endpoint").value = config.sources.admin_api_endpoint ?? "";

  renderProfiles();
}

function renderProfiles() {
  const list = document.getElementById("profilesList");
  list.innerHTML = config.profiles
    .map(
      (p, i) => `
    <div class="profile-row">
      <label><span>ID</span><input data-i="${i}" data-f="id" value="${p.id}"></label>
      <label><span>Name</span><input data-i="${i}" data-f="name" value="${p.name}"></label>
      <label><span>Color</span><input data-i="${i}" data-f="color" type="color" value="${p.color}"></label>
      <label><span>Plan</span>
        <select data-i="${i}" data-f="plan_type">
          <option${p.plan_type === "Pro" ? " selected" : ""}>Pro</option>
          <option${p.plan_type === "Max" ? " selected" : ""}>Max</option>
          <option${p.plan_type === "Team" ? " selected" : ""}>Team</option>
          <option${p.plan_type === "Enterprise" ? " selected" : ""}>Enterprise</option>
          <option${p.plan_type === "Api" ? " selected" : ""}>Api</option>
        </select>
      </label>
      <label><span>5-hour limit</span><input data-i="${i}" data-f="five_hour_limit" type="number" value="${p.five_hour_limit}"></label>
      <label><span>Admin API key</span><input data-i="${i}" data-f="admin_key_value" type="password" placeholder="${p.admin_api_key_ref ? "•••• (set)" : "unset"}"></label>
      <button data-i="${i}" data-act="remove">Remove</button>
    </div>`
    )
    .join("");

  list.querySelectorAll("input[data-i], select[data-i]").forEach((el) => {
    el.addEventListener("change", () => {
      const i = +el.dataset.i;
      const f = el.dataset.f;
      const val = el.type === "number" ? +el.value : el.value;
      if (f === "admin_key_value") {
        // Stored separately; ref key is set when we save.
        config.profiles[i]._pending_admin_key = val;
      } else {
        config.profiles[i][f] = val;
      }
    });
  });
  list.querySelectorAll("button[data-act='remove']").forEach((el) => {
    el.addEventListener("click", () => {
      config.profiles.splice(+el.dataset.i, 1);
      renderProfiles();
    });
  });
}

document.getElementById("addProfile").addEventListener("click", () => {
  config.profiles.push({
    id: "new" + Date.now(),
    name: "new",
    color: "#888780",
    admin_api_key_ref: null,
    plan_type: "Pro",
    five_hour_limit: 50000,
    weekly_limit: null,
  });
  renderProfiles();
});

document.querySelectorAll(".tab").forEach((t) => {
  t.addEventListener("click", () => {
    document.querySelectorAll(".tab").forEach((x) => x.classList.remove("on"));
    document.querySelectorAll(".panel").forEach((x) => x.classList.remove("on"));
    t.classList.add("on");
    document.querySelector(`.panel[data-panel="${t.dataset.tab}"]`).classList.add("on");
  });
});

document.getElementById("saveBtn").addEventListener("click", async () => {
  for (const k of ["render_interval_ms", "admin_poll_interval_s", "otel_port"]) {
    config.app[k] = +document.getElementById(k).value;
  }
  for (const k of [
    "minimize_to_tray",
    "autostart",
    "always_on_top_default",
    "jingle_on_launch",
    "notification_sound",
    "notification_toasts",
    "red_mode_enabled",
  ]) {
    config.app[k] = document.getElementById(k).checked;
  }
  for (const k of Object.keys(config.thresholds)) {
    config.thresholds[k] = +document.getElementById(k).value;
  }
  const endpoint = document.getElementById("admin_api_endpoint").value.trim();
  config.sources.admin_api_endpoint = endpoint || null;

  for (const p of config.profiles) {
    if (p._pending_admin_key) {
      const keyRef = `tokenman-profile-${p.id}`;
      await invoke("cmd_set_admin_key", { args: { key_ref: keyRef, value: p._pending_admin_key } });
      p.admin_api_key_ref = keyRef;
      delete p._pending_admin_key;
    }
  }

  await invoke("cmd_save_config", { config });
  getCurrentWindow().close();
});

document.getElementById("cancelBtn").addEventListener("click", () => getCurrentWindow().close());
document.getElementById("testAlert").addEventListener("click", () => invoke("cmd_test_alert"));
document.getElementById("checkUpdate").addEventListener("click", () => invoke("cmd_refresh_registry"));

load().catch((e) => console.error("settings load failed", e));
