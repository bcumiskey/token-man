// Token-Man frontend — webview half.
//
// Subscribes to `state-update` events from the Rust backend and renders into
// the HUD DOM. All user actions round-trip through IPC commands.

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

// --- DOM refs ---
const splash = document.getElementById("splash");
const hud = document.getElementById("hud");
const tokPerMin = document.getElementById("tokPerMin");
const liveCount = document.getElementById("liveCount");
const costToday = document.getElementById("costToday");
const limitEta = document.getElementById("limitEta");
const burnRate = document.getElementById("burnRate");
const cacheRate = document.getElementById("cacheRate");
const ctxFill = document.getElementById("ctxFill");
const ctxVal = document.getElementById("ctxVal");
const fiveFill = document.getElementById("fiveFill");
const fiveVal = document.getElementById("fiveVal");
const cacheFill = document.getElementById("cacheFill");
const cacheVal = document.getElementById("cacheVal");
const weekFill = document.getElementById("weekFill");
const weekVal = document.getElementById("weekVal");
const stateBadge = document.getElementById("stateBadge");
const profileName = document.getElementById("profileName");
const profileSwatch = document.getElementById("profileSwatch");
const srcCount = document.getElementById("srcCount");
const regCell = document.getElementById("regCell");
const sessionRows = document.getElementById("sessionRows");
const alertStrip = document.getElementById("alertStrip");
const vuNeedle = document.getElementById("vuNeedle");
const pinBtn = document.getElementById("pinBtn");

// --- Spectrum bars ---
function makeBars(rowEl, count) {
  const wrap = document.createElement("div");
  wrap.style.cssText = "flex:1;height:100%;display:flex;align-items:flex-end;gap:1px;margin-left:18px;position:relative;";
  const arr = [];
  for (let i = 0; i < count; i++) {
    const cell = document.createElement("div");
    cell.style.cssText = "position:relative;flex:1;height:100%;";
    const bar = document.createElement("div");
    bar.className = "wa-bar";
    bar.style.height = "1px";
    const peak = document.createElement("div");
    peak.className = "wa-peak";
    peak.style.bottom = "1px";
    cell.appendChild(bar);
    cell.appendChild(peak);
    wrap.appendChild(cell);
    arr.push({ bar, peak, peakVal: 0 });
  }
  rowEl.appendChild(wrap);
  return arr;
}
const BAR_COUNT = 40;
const inBars = makeBars(document.getElementById("vizIn"), BAR_COUNT);
const outBars = makeBars(document.getElementById("vizOut"), BAR_COUNT);

// --- LED strip ---
const ledStrip = document.getElementById("ledStrip");
const leds = [];
for (let i = 0; i < 16; i++) {
  const el = document.createElement("div");
  el.className = "wa-led";
  ledStrip.appendChild(el);
  leds.push(el);
}

// --- VU needle state (ballistic animation) ---
let vuTargetTps = 0;
let vuAngle = -45;
const vuHistory = []; // {t, tps}
const VU_HISTORY_MS = 60_000;

function vuFrame() {
  const now = performance.now();
  // Track recent samples for auto-scaling (95th percentile over last 60s).
  if (vuHistory.length === 0 || now - vuHistory[vuHistory.length - 1].t > 100) {
    vuHistory.push({ t: now, tps: vuTargetTps });
    while (vuHistory.length && now - vuHistory[0].t > VU_HISTORY_MS) vuHistory.shift();
  }
  const vals = vuHistory.map((h) => h.tps).sort((a, b) => a - b);
  const p95 = vals.length ? vals[Math.floor(vals.length * 0.95)] : 0;
  const scale = Math.max(10, p95 / 0.8);
  const norm = Math.min(1.1, vuTargetTps / scale);
  const jitter = (Math.random() - 0.5) * 2.5;
  const targetAngle = -45 + norm * 90 + jitter;
  // Ballistic: faster attack than decay.
  const diff = targetAngle - vuAngle;
  const rate = diff > 0 ? 0.25 : 0.08;
  vuAngle += diff * rate;
  vuNeedle.setAttribute("transform", `rotate(${vuAngle.toFixed(2)} 70 50)`);
  const intensity = (vuAngle + 45) / 90;
  for (let i = 0; i < leds.length; i++) {
    const threshold = (i + 1) / leds.length;
    const active = intensity >= threshold - 0.05;
    leds[i].className = "wa-led";
    if (active) leds[i].classList.add(i >= 12 ? "amber" : "on");
  }
  requestAnimationFrame(vuFrame);
}
requestAnimationFrame(vuFrame);

// --- State render ---
let expandedSession = null;

function fmtNum(n) {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(2) + "M";
  return Math.round(n).toLocaleString();
}

function classForPercent(v, warn = 70, crit = 85, invert = false) {
  if (invert) {
    if (v <= 100 - crit) return "crit";
    if (v <= 100 - warn) return "warn";
    return "";
  }
  if (v >= crit) return "crit";
  if (v >= warn) return "warn";
  return "";
}

function renderState(st) {
  hud.dataset.state = st.globalState;

  tokPerMin.textContent = fmtNum(st.metrics.tokensPerMin);
  liveCount.textContent = String(st.metrics.liveSessions);
  costToday.textContent = "$" + st.metrics.costToday.toFixed(2);
  limitEta.textContent = st.metrics.limitEta ?? "— — —";
  burnRate.textContent = "$" + st.metrics.burnPerHour.toFixed(2) + "/hr";
  cacheRate.textContent = Math.round(st.metrics.cacheHitRate) + "%";

  function setMeter(fill, val, v, warn, crit, inverted) {
    const pct = Math.min(100, Math.max(0, v));
    fill.style.width = pct + "%";
    let cls = "";
    if (inverted) {
      // Lower is worse (cache hit rate). warn/crit are raw thresholds:
      // e.g. warn=80, crit=70 → amber when <warn, red when <crit.
      if (pct < crit) cls = "crit";
      else if (pct < warn) cls = "warn";
    } else {
      if (pct >= crit) cls = "crit";
      else if (pct >= warn) cls = "warn";
    }
    fill.className = "wa-meter-fill" + (cls ? " " + cls : "") + (fill === weekFill ? " blue" : "");
    val.textContent = Math.round(pct) + "%";
    val.className = "wa-meter-val" + (cls ? " " + cls : "");
  }
  const cacheWarn = st.thresholds?.cacheWarn ?? 80;
  const cacheCrit = st.thresholds?.cacheCritical ?? 70;
  setMeter(ctxFill, ctxVal, st.metrics.ctxWorst, 70, 85, false);
  setMeter(fiveFill, fiveVal, st.metrics.fiveHour, 70, 85, false);
  setMeter(cacheFill, cacheVal, st.metrics.cacheHitRate, cacheWarn, cacheCrit, true);
  setMeter(weekFill, weekVal, st.metrics.week, 70, 85, false);

  const stateText = { ok: "OK", warn: "WARN", alert: "ALERT" }[st.globalState] ?? "OK";
  stateBadge.textContent = `${stateText} · ${st.metrics.liveSessions} LIVE`;

  profileName.textContent = st.profile.name;
  profileSwatch.style.background = st.profile.color;

  regCell.textContent = `REG ${st.registry.version} · ${st.registry.updatedAt}`;
  srcCount.textContent = `${st.sources.length} SRC · ${st.metrics.liveSessions} LIVE`;

  pinBtn.classList.toggle("active", st.pinActive);

  // Spectrum
  const n = Math.min(BAR_COUNT, st.spectrum.in.length);
  const pad = BAR_COUNT - n;
  for (let i = 0; i < BAR_COUNT; i++) {
    const idx = i - pad;
    const vi = idx >= 0 ? st.spectrum.in[idx] : 0;
    const vo = idx >= 0 ? st.spectrum.out[idx] : 0;
    const hi = Math.max(1, vi * 18);
    const ho = Math.max(1, vo * 18);
    inBars[i].bar.style.height = hi.toFixed(1) + "px";
    outBars[i].bar.style.height = ho.toFixed(1) + "px";
    if (hi > inBars[i].peakVal) inBars[i].peakVal = hi;
    else inBars[i].peakVal = Math.max(1, inBars[i].peakVal - 0.4);
    if (ho > outBars[i].peakVal) outBars[i].peakVal = ho;
    else outBars[i].peakVal = Math.max(1, outBars[i].peakVal - 0.4);
    inBars[i].peak.style.bottom = inBars[i].peakVal.toFixed(1) + "px";
    outBars[i].peak.style.bottom = outBars[i].peakVal.toFixed(1) + "px";
  }

  // VU needle target — ballistic animation handled in rAF loop below.
  vuTargetTps = st.metrics.tokensPerSec;

  // Sessions
  renderSessions(st.sources);

  // Alerts strip
  if (st.alerts.length === 0) {
    alertStrip.innerHTML = '<span class="wa-hist-item" style="color:var(--wa-text-dim)">no recent events</span>';
  } else {
    alertStrip.innerHTML = st.alerts
      .map((a) => {
        const inverted = a.kind === "cache_low" || a.kind === "limit_eta";
        const intense = inverted
          ? a.value <= a.threshold * 0.9
          : a.value >= a.threshold * 1.1;
        const color = a.resolved ? "g" : intense ? "r" : "a";
        const label = `${a.kind}${a.sourceId ? " " + a.sourceId : ""} ${Math.round(a.value)}`;
        return `<span class="wa-hist-item"><span class="wa-hist-dot ${color}"></span>${label}</span>`;
      })
      .join("");
  }
}

function renderSessions(sources) {
  if (sources.length === 0) {
    sessionRows.innerHTML = '<div class="wa-sess-empty">no sources yet — waiting for activity</div>';
    return;
  }
  sources.sort((a, b) => a.id.localeCompare(b.id));
  const html = sources
    .map((s) => {
      const dotCls = s.isOpaque
        ? "opaque"
        : s.status === "active"
        ? ""
        : s.status === "idle"
        ? "idle"
        : s.status === "stale"
        ? "stale"
        : "idle";
      const ctx = s.contextPercent != null ? Math.round(s.contextPercent) + "%" : "—";
      const tpm = s.tokensPerMin != null ? fmtNum(s.tokensPerMin) : "—";
      const proj = s.project ?? (s.isOpaque ? "opaque (plan agg)" : "—");
      const expandedHtml =
        expandedSession === s.id
          ? `<div class="wa-sess-detail">
               <span>cache ${s.cacheHitRate != null ? Math.round(s.cacheHitRate) + "%" : "—"}</span>
               <span>status ${s.status}</span>
             </div>`
          : "";
      return `<div class="wa-sess-row${expandedSession === s.id ? " expanded" : ""}" data-id="${s.id}">
        <span class="wa-sess-dot ${dotCls}"></span>
        <span class="wa-sess-src">${escapeHtml(s.kind)}</span>
        <span class="wa-sess-model">${escapeHtml(shortModel(s.model))}</span>
        <span>${escapeHtml(proj)}</span>
        <span>${ctx}</span>
        <span>${tpm}</span>
        <span>$${s.costToday.toFixed(2)}</span>
        <span class="wa-sess-owner">${escapeHtml(s.owner)}</span>
        ${expandedHtml}
      </div>`;
    })
    .join("");
  sessionRows.innerHTML = html;

  sessionRows.querySelectorAll(".wa-sess-row").forEach((row) => {
    row.addEventListener("click", () => {
      const id = row.dataset.id;
      expandedSession = expandedSession === id ? null : id;
    });
  });
}

function shortModel(m) {
  return m.replace(/^claude-/, "").replace(/-20\d{6}$/, "");
}

function escapeHtml(s) {
  return String(s ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));
}

// --- IPC wiring ---
let firstTick = true;
listen("state-update", (e) => {
  renderState(e.payload);
  if (firstTick) {
    firstTick = false;
    setTimeout(() => splash.classList.add("hide"), 1000);
    setTimeout(() => (splash.style.display = "none"), 1500);
    hud.hidden = false;
  }
}).catch(console.error);

listen("alert-fired", () => {
  // Visual cue — red pulse is already driven by globalState.
});

// Buttons
document.getElementById("pinBtn").addEventListener("click", () => invoke("cmd_toggle_pin"));
document.getElementById("minBtn").addEventListener("click", () => invoke("cmd_minimize"));
document.getElementById("closeBtn").addEventListener("click", () => invoke("cmd_close"));
document.getElementById("settingsBtn").addEventListener("click", () => invoke("cmd_open_settings"));
document.getElementById("snapBtn").addEventListener("click", async () => {
  await invoke("cmd_snap_to_clipboard");
  flash(document.getElementById("snapBtn"), "COPIED");
});
document.querySelectorAll("#windowBtns button[data-win]").forEach((btn) => {
  btn.addEventListener("click", () => {
    document.querySelectorAll("#windowBtns button").forEach((b) => b.classList.remove("on"));
    btn.classList.add("on");
    invoke("cmd_set_spectrum_window", { window: btn.dataset.win });
  });
});
regCell.addEventListener("click", async () => {
  try {
    const r = await invoke("cmd_refresh_registry");
    flash(regCell, `REG ${r.version}`);
  } catch (e) {
    flash(regCell, "REG FAIL");
  }
});

function flash(el, text) {
  const prev = el.textContent;
  el.textContent = text;
  setTimeout(() => (el.textContent = prev), 1200);
}
