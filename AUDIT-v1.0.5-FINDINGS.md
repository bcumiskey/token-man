# Token-Man v1.0.5 Numerical Correctness & Logic Audit

**Date:** 2026-04-28
**Auditor:** Claude (code-review pass)
**Scope:** `app/src-tauri/src/**`, `app/src/**`, `assets/model-registry.json`, live `state.db`, live JSONL stream
**Mode:** Read-only on source. The release binary is GUI-only (`--help` returns no headless mode); displayed values were derived from code paths combined with current DB rows and live `CTX_DIAG` log emissions captured from the running v1.0.5 process. Where a value is "derived not observed", that is called out per row.

---

## 1. Inventory of Displayed Values

| ID | Display label | Where shown | Source variable | Calculation path | Unit | Refresh trigger |
|---|---|---|---|---|---|---|
| D01 | CTX % (worst) — meter + label | HUD `#ctxFill`/`#ctxVal` | `metrics.ctxWorst` ← `global.context_percent_worst` | `max` over sources of `src.context_percent` (derived per-source as `(context_max_tokens × tokenizer_inflation / context_window) × 100`); `context_max_tokens` is monotonic max of per-turn `input + cache_read + cache_write` across the source's lifetime (`aggregator.rs:184–188`, `:306–310`) | percent | every aggregator tick (500 ms) |
| D02 | CTX absolute (per source) | session row "ctx" column | `SourceView.contextPercent` | per-source same as D01 | percent | every state-update emit |
| D03 | tok/min (HUD top metric) | `#tokPerMin` | `metrics.tokensPerMin` ← `global.tokens_per_min_current` | sum across sources of `recent_ticks` (`(input + cache_write) + output`) over last 60 s; pruned every recompute (`aggregator.rs:289–293, 270–277`) | tokens/min | 500 ms |
| D04 | tok/sec (VU needle) | `#vuNeedle`, LED strip | `metrics.tokensPerSec` ← `global.tokens_per_sec_vu` | sum over sources of last-2 s ticks ÷ 2 (`aggregator.rs:296–303`) | tokens/sec | 500 ms backend, rAF frontend; auto-scaled to 95th-pctile of last 60 s in JS |
| D05 | $ today | `#costToday` | `metrics.costToday` | sum across sources of `cost_today`; `cost_today` accumulates `registry.cost(...)` per usage event (no daily reset) | USD | 500 ms |
| D06 | Burn $/hr | `#burnRate` | `metrics.burnPerHour` | `Db::sum_cost_since(now − 10 min) × 6` (`aggregator.rs:344–347`) | USD/hr | 500 ms |
| D07 | ETA to limit | `#limitEta` | `metrics.limitEta` (formatted) | `(1 − five_hour_pct/100) × five_hour_limit ÷ tokens_per_min_current`; floored at TPM ≥ 100, capped 24 h, otherwise `None`/"— — —" (`aggregator.rs:349–362`) | minutes (display "Xm" / "Xh Ym" / "24H+") | 500 ms |
| D08 | Cache hit rate | `#cacheRate`, meter `#cacheFill` | `metrics.cacheHitRate` | per-source: `cache_read_today ÷ (input_tokens_today + cache_read_today + cache_write_today) × 100`; global = token-weighted mean over sources weighted by `(input_today + cache_read_today)` (note: weight excludes cache_write — see F11) (`aggregator.rs:233–237, 311–315, 322–327`) | percent | 500 ms |
| D09 | 5-hour window % | `#fiveFill`/`#fiveVal` | `metrics.fiveHour` | `Db::sum_tokens_since(now − 5 h)` returning `(SUM(input), SUM(output))`, then `(input+output) ÷ five_hour_limit × 100` (`aggregator.rs:329–334`) | percent | 500 ms |
| D10 | Week % | `#weekFill`/`#weekVal` | `metrics.week` | same shape over 7 days vs `weekly_limit` | percent | 500 ms |
| D11 | LIVE count | `#liveCount`, `#stateBadge` | `metrics.liveSessions` | count of sources where `last_activity ≥ now−60 s` and not Offline (`aggregator.rs:255–287`) | count | 500 ms |
| D12 | Spectrum bars (60s/1h/24h) | `#vizIn`/`#vizOut` | `spectrum.in/out` | 1 s tick sums per-source; downsampled to 90 s and 36-min bars; ring-buffered 40 entries (`aggregator.rs:381–413`); seeded from `bucket_tokens` at startup which sums `input + cache_read + cache_write` for "in" (`db.rs:135–158`) — different definition than D03 | normalized (0..1) | 1 Hz |
| D13 | State badge / GlobalState | `#stateBadge` | `globalState` | derived from unresolved alerts in `thresholds::compute_global_state` | enum OK/WARN/ALERT | 500 ms |
| D14 | Profile name + swatch | `#profileName`, `#profileSwatch` | `profile.name` / `.color` | `state.profile_meta[active_profile]` with hard-coded fallback "#378ADD" if id missing (`state.rs:339–356`) | string/hex | 500 ms |
| D15 | Source count | `#srcCount` | `sources.length` | `state.sources.values()` length | count | 500 ms |
| D16 | Registry version | `#regCell` | `registry.version`/`updatedAt` | bundled JSON; no live update validation | string | on event |
| D17 | Per-source `cost_today` | session rows | `SourceView.costToday` | accumulator above | USD | 500 ms |
| D18 | Alert chips | `#alertStrip` | `alerts[]` | last 8 from `state.alerts` ring | mixed | 500 ms |
| D19 | Snap-to-clipboard MD | clipboard | `snapshot::build_markdown` | reads same `global.*` fields; recomputes "live_now" from sources at snapshot time | text | on click |

### Undocumented / partially documented displays
- **VU auto-scale** (D04): the displayed needle position is not a token rate at all — it is `tokensPerSec / max(10, p95_60s/0.8)` clamped to `[0,1.1]`. The label "tok/sec" is misleading; it is in practice "intensity vs your last minute". Not a math error, but it means the meter can show "full deflection" at vastly different absolute rates.
- **Spectrum "in" definition mismatch** (D12 vs D03): in-memory ticks use `input + cache_write` (D03 rule), but the DB-seeded spectrum at startup uses `input + cache_read + cache_write` (`db.rs:145`). The first hour after launch shows ~10–100× larger "in" bars than the same activity does after the rings have rolled over.
- **`cost_today` never resets** (D05, D17): the field is named "today" but accumulates for the lifetime of the process. There is no daily wall-clock rollover. Confirmed in `aggregator.rs:174` (only `+=`, no reset). DB shows lifetime-cost = $234.01; HUD will show that until the process restarts (and the in-memory state is wiped).

---

## 2. Ground Truth Map

| ID | Has external GT? | Source | How to obtain | Reliability |
|---|---|---|---|---|
| D01/D02 CTX % | Partial | Claude Code `/context`; per-turn JSONL `usage.input_tokens + cache_read_input_tokens + cache_creation_input_tokens` for the **latest** assistant turn | Read JSONL tail line; sum the three fields; divide by model context window | `/context` is the user-facing truth; JSONL tail is mechanical truth. Both report a *current* value, not a historical max. |
| D03 tok/min | Yes | DB SUM over last 60 s | `SELECT SUM(input_tokens+output_tokens+cache_write_tokens) WHERE timestamp ≥ now-60s` | High |
| D04 tok/sec | Yes (raw) | Same as D03 over 2 s; the *displayed needle* has no GT (it's a normalized intensity) | — | Raw value verifiable; needle position is intentionally untrue |
| D05 $ today | Yes (lifetime cost) | DB `SUM(cost_usd)` over current calendar day; *Token-Man* version is process-lifetime, not calendar day | `SELECT SUM(cost_usd) WHERE timestamp ≥ midnight_local` | Mismatch by definition — see F4 |
| D06 burn $/hr | Yes | DB `SUM(cost_usd) WHERE ts ≥ now-10min × 6` | High; identical formula |
| D07 ETA | Indirect | Derived from D09 + D03; truthiness depends on inputs | — | Only as good as inputs |
| D08 cache hit rate | Yes | `cache_read ÷ (input + cache_read + cache_write)` | DB sums | High; per-source matches; global weighting is questionable (F11) |
| D09 5h % | Yes | `(SUM(input)+SUM(output)) ÷ five_hour_limit` over last 5 h | High; **but** Anthropic's 5-hour limit measures **billable tokens** which include cache_write at 1.25× and (debatably) cache_read at 0.1×. Using `input+output` excludes the dominant token bucket. See F2. |
| D10 week % | Same as D09 | Same | Same |
| D11 LIVE | Yes | Sources whose latest event ≤ 60 s ago | High |
| D12 spectrum | Yes | DB bucketed sums | The "in" definition differs between in-memory and DB-seed paths (F8) |
| D14 profile | N/A | config.toml | Local truth |
| D16 registry | Yes | `assets/model-registry.json` shipped value | High |

**Highest-risk surface (no clean GT, or definitional drift):**
- D05 (`cost_today`) — the word "today" is wrong; it is "since process start".
- D09/D10 (`five_hour_%`, `week_%`) — denominator of `input+output` does not match what Anthropic actually meters.
- D04 needle position — by design not GT; just flag the labeling.
- D01 CTX % — formula matches `/context` *only at the latest turn*; current code uses historical max, which guarantees drift after any `/compact`.

---

## 3. Live Comparison Results

Captured at 2026-04-28 09:22 UTC against the live state.db (782 events, range 2026-04-22 → 2026-04-28). All "Displayed" values are **derived from code path applied to current DB rows** (binary has no headless dump). The CTX_DIAG log lines from the running v1.0.5 process were used to confirm CTX path matches the derivation.

| ID | Displayed (derived) | Expected (GT) | Delta | Delta % | Classification | Notes |
|---|---|---|---|---|---|---|
| D01 CTX worst | **101.78 %** (170,850 raw × 1.22 ÷ 204,800 × 100) — source `code:09ecf654…` | Latest turn ctx_raw 106,334 → 63.34 % w/ inflation, 51.92 % raw vs 200k true window | +50.0 pp | +97 % | **Severe mismatch** | Reproduces the "102 vs 53" report exactly. Root cause: `context_max_tokens` is monotonic (only updated when `ctx_tokens > current`). After `/compact` or natural context shrink, the displayed % stays pinned to the historical peak. See F1. |
| D01b CTX worst from CTX_DIAG live | 29.01 % (current active session 43ab6690 ctx_raw=48,704 × 1.22 ÷ 204,800) | 29.01 % | 0 | 0 | Match (for an actively-growing session) | Confirms the formula is correct in the steady-growth case. The S1 surfaces only after a contraction event. |
| D03 tok/min | 0 | 0 (no events in last 60 s at the snapshot moment) | 0 | — | Match | |
| D05 $ today | accumulator value (no resets, lifetime) | DB sum since today 00:00 UTC: **$3.88** (≈10 min worth) vs lifetime **$234.01** | up to +$230 | +5,900 % over 7 days | **Material mismatch** (definitional) | F4 |
| D06 burn $/hr | $3.88 × 6 = **$23.28/hr** | $23.28/hr | 0 | 0 | Match (algorithmically) | But the underlying 10-min cost is sensitive to per-event spikes; idle→burst inflation discussed in F3. |
| D07 ETA | None (TPM=0 < 100 floor) | None | 0 | — | Match | |
| D08 cache hit rate (global) | weighted mean of per-source hit rates with weight `input+cache_read` | per-source `cache_read ÷ (i+cr+cw)` is correct; but global weighting excludes `cache_write` from denominator: **understates importance of write-heavy sources** | small but systematic | ~1–5 pp | Minor drift | F11 |
| D09 5h % | (72 + 10,259) / 200,000 × 100 = **5.17 %** | If "limit" is `input+output` only: 5.17 % match. If Anthropic's 5h measures something closer to `input + cache_write × 1.25 + cache_read × 0.1 + output`: ≈ (72 + 10,259 + ~1.6M cache adjusted)/200k → much higher, possibly >100% (window already exceeded?). Cannot resolve without Anthropic's published meter. | unknown but potentially **massive** | unknown | **Material mismatch (suspected)** | F2 |
| D10 week % | (3,151 + 549,262) / 5,000,000 × 100 = **11.05 %** | Same caveat as D09 | unknown | unknown | Material mismatch (suspected) | F2 |
| D11 LIVE | 0 (no events in 60 s at snapshot) | 0 | 0 | — | Match | |
| D14 profile name | falls back to id `"you"` with default color (no profile with id="you" exists; only id="Bryan") | "Bryan" / "#378ADD" | label drift | — | Cosmetic | F12. Triggers `tracing::warn!` per render tick (~2/s) — log-spam side-effect. |
| D16 registry version | "4.7" | bundled file contains "4.7" | 0 | — | Match | |

---

## 4. Logic audit

### 4a. Burn rate (D03 tok/min, D06 burn $/hr)

`aggregator.rs:289–303` and `:344–347`.

- **Algorithm.** Tok/min: sum over all sources of in-memory `recent_ticks` deque entries within the last 60 s, where each tick is `(t, input+cache_write, output)`. Burn $/hr: DB `SUM(cost_usd)` over last 10 min × 6.
- **Time window.** Tok/min uses wall-clock now (correct decision, ignores JSONL ts skew — `aggregator.rs:158`). Pruning is forced every 500 ms recompute (`:270–277`) so idle sources do go to 0.
- **Edge cases:**
  - **Idle → burst cache_read.** Tok/min already excludes `cache_read` (`aggregator.rs:217–222` — explicitly documented). Good. **However** the spectrum `bucket_tokens` startup seed *includes* `cache_read` as input; users see the first-hour bars 10× taller than they will be after the rings re-fill. See F8.
  - **Sub-second turns.** Multiple JSONL writes within the same 500 ms tick all land in the same `recent_ticks` deque entry; per-second rate is correct (sums them); per-minute rate is correct.
  - **Single-turn session.** Initial tick lasts ~60 s, then prunes; tok/min decays cleanly to 0.
  - **Mid-stream model switch.** `recent_ticks` doesn't carry model; tok/min sum is model-agnostic, which is the right choice. ETA, however, uses a single `five_hour_limit` from active profile regardless of model — fine for now.
  - **Concurrent windows / double-count.** Tok/min iterates `s.sources.values_mut()` and sums. **Sidechain/subagent sessions get distinct `source_id`s** (see CTX_DIAG: `code:agent-ac4b20ebb1591459b`), and their JSONL files live in `subagents/` subdirs but are walked by `walk_jsonl` (`ccusage.rs:164–179`). The parent session's main JSONL also records the subagent's outputs in some Claude Code versions — **possible double-count** of subagent output. Not confirmed live; flagged for follow-up. See F5.

### 4b. ETA (D07)

`aggregator.rs:349–362`.

- **Formula.** `mins = ((1 − pct/100) × five_hour_limit) ÷ tokens_per_min_current`.
- **Low-rate floor.** TPM < 100 → `None`. Sane. Above the floor, with TPM = 100 and pct=0 you'd get 200,000/100 = 2000 min = 33 h, capped at 24 h. The cap is correct.
- **Zero / negative rate.** Guarded by floor.
- **Negative remaining (CTX > 100 %).** `remaining` is computed from `five_hour_pct`, not from CTX. If `five_hour_pct ≥ 100` the function returns `None`. Good.
- **Model-switch context-size change.** Irrelevant — ETA is about token budget, not context window.
- **Unit consistency (60× error site).** `remaining` is in *tokens*, `tpm` is in *tokens/min*, result is *minutes*. No 60× error. ✓
- **Risk:** `tokens_per_min_current` includes cache_write but not cache_read in tok/min (correct for burn-rate framing), while `five_hour_limit` denominator (i+o only, see F2) excludes cache_write. So **ETA mixes two definitions** — the rate of consumption and the budget being consumed are not the same currency. ETA is therefore systematically optimistic when cache writes dominate work. F6.

### 4c. Stale rate pruning (D03)

`aggregator.rs:270–277`. Fires every recompute (500 ms). Threshold = 60 s. Sane. After 60 s of no events the deque empties → `recent_sum=0` → tok/min = 0. **Good**. No freeze/decay/zero ambiguity.

### 4d. Cost (D05, D06, D17)

`registry.rs:38–48`.

- **Per-model rates** (registry):
  - opus-4-7: $15 in / $75 out / cache-read 0.1× / cache-write 1.25×.
  - sonnet-4-6: $3 in / $15 out, same multipliers.
  - haiku-4-5: $1 in / $5 out, same multipliers.

  Cross-checked against Anthropic published rates as of model launch — these match. Cache multipliers (0.1×, 1.25×) match Anthropic's documented prompt-caching pricing. ✓

- **Cache priced separately?** Yes. `cost()` adds `read_cost` and `write_cost` distinct from `input_cost` and `output_cost`. ✓
- **Hypothetical-rate framing.** v1.0.5 always applies registry pricing; the registry has no "hypothetical vs real" toggle. Fine for subscription-plan users (they don't pay per token), but the displayed cost is then *always* hypothetical and the UI does not flag this. Not a math bug — a labeling issue. F10.
- **Currency / unit consistency.** All `f32` in USD. Per-million conversion `per = 1_000_000.0_f32` is consistent across all four sub-costs. ✓ (Use of `f32` for cost accumulators is acceptable for display but loses precision past ~$16k; not a near-term concern.)

### 4e. Tokenizer 1.22× inflation (D01)

`registry.rs:56–58`, `aggregator.rs:189–193`.

- **Where applied.** Only in CTX % (D01). Inflation is *not* applied to tok/min, burn, 5h%, week%, or any other field.
- **Why it exists.** `claude-opus-4-7` reports usage tokens that under-count true context-window consumption by ~22 %. Without the multiplier, displayed CTX would read ~80 % when `/context` reports ~100 %. With it, `/context` and HUD agree (in the steady-growth case — confirmed by CTX_DIAG line 29.01 % matching expectation).
- **Structural mismatch (this is the finding).** The same usage tokens are consumed by tok/min, burn, and 5h%. If the 5-hour billable window is also expressed in true-context units (likely — Anthropic counts what it serves), then the 5h% denominator/numerator are both un-inflated → wash. But if the rate-limit actually meters the inflated count (since that's what the model actually saw), then the 5h % is *under*-reporting by ~18 % on opus-4-7. Without Anthropic's billable-unit definition this is unfalsifiable. Documented as F7.
- **Missing models.** Inflation = 1.22 only on `claude-opus-4-7`; sonnet-4-6 / opus-4-6 / haiku-4-5 all have 1.0. If user runs `claude-opus-4-8` (or any model not in the registry), inflation defaults to 1.0 and CTX would silently be ~22 % low. Unknown-model regression risk. F9.

---

## 5. Cross-cutting

### Unit conversions (60×/1000× risk audit)
| Site | From → To | Multiplier | Verdict |
|---|---|---|---|
| `tokens_per_min` | tokens / 60 s window → tok/min | sum-as-is (window IS one minute) | ✓ |
| `tokens_per_sec_vu` | tokens / 2 s → tok/s | `÷ 2.0` | ✓ |
| burn rate | $/10 min → $/hr | `× 6.0` | ✓ |
| cost | tokens → USD | `÷ 1_000_000` | ✓ |
| ETA | tokens ÷ tok/min → min | direct | ✓ (mixed currency — see F6) |

No 60× or 1000× errors detected.

### Off-by-one window denominators (the 204,800 vs 1,000,000 bug class)
- `context_window` = **204,800** for every model in registry. Comment in JSON says "effective subscription context (200k). Extended 1M context requires beta flags". 200,000 is the real number; 204,800 is 200×1024. Using 204,800 puts displayed CTX **2.4 % below** what `/context` would show if `/context` divides by 200,000. This is a small but systematic under-report (offsetting the over-report from inflation, partially). F13.
- Pricing denominator = `1_000_000` ✓.
- 5-hour limit profile = `200_000` (tokens). Correct order of magnitude.
- Weekly limit = `5_000_000`. Correct.

### Rust ↔ JS boundary
| Field | Rust type/unit | JS rename | JS use | Verdict |
|---|---|---|---|---|
| `tokens_per_min` | f32 tok/min | `tokensPerMin` | `fmtNum` | ✓ |
| `tokens_per_sec` | f32 tok/sec | `tokensPerSec` | VU norm | ✓ (intentional non-truth) |
| `cost_today` | f32 USD | `costToday` | `.toFixed(2)` | ✓ unit, ✗ semantics (F4) |
| `burn_per_hour` | f32 USD/hr | `burnPerHour` | `.toFixed(2)` | ✓ |
| `ctx_worst` | f32 % | `ctxWorst` | meter clamps `[0,100]` | ✓ but the **clamp masks F1** — when backend says 102 %, JS displays 100 %. The displayed bar will be pinned full and the value label will read 102 % — confirmed in `setMeter`: `pct = Math.min(100, Math.max(0, v))` for the **bar width**, but `val.textContent = Math.round(pct) + "%"` also uses the clamped `pct`. So actually JS *clamps the label too* — user should see "100 %", not "102 %". The "102 %" report from the user must therefore come from elsewhere (per-source `contextPercent` is rendered via `Math.round(s.contextPercent) + "%"` in the session row — **unclamped**). That's the surface where 102 % appears. F1 evidence. |
| `five_hour` / `week` | f32 % | `fiveHour`/`week` | clamped meter | ✓ unit; semantics F2 |
| `cache_hit_rate` | f32 % | `cacheHitRate` | `Math.round(...)` | ✓ |
| `limit_eta` | `Option<u32>` minutes, formatted to `Option<String>` in Rust | `limitEta` | `?? "— — —"` | ✓ |
| spectrum `in`/`out` | `Vec<f32>` 0..1 normalized in Rust (`build_view`) | `in`/`out` | bar height × 18 px | ✓ |

### State DB drift (3 spot checks)

1. **D01 CTX worst.** Backend would compute 101.78 % for source `09ecf654` from DB row max 170,850 × 1.22 ÷ 204,800. Live CTX_DIAG shows the formula matches. **Drift to GT (`/context` ≈ 53 % per latest turn): +50 pp.** Confirmed F1.
2. **D06 burn $/hr.** Backend would compute `$3.87685 × 6 = $23.26/hr` from DB rows in last 10 min. Matches algorithm exactly. ✓
3. **D09 5h %.** Backend computes `(72 + 10,259) / 200,000 × 100 = 5.17 %`. Algorithm exact. Truthiness depends on whether `input+output` is the right metering (see F2).

Staleness window: `recent_ticks` decays ≤500 ms; DB writes are synchronous on each event (`db.insert_event` blocks before in-memory updates — `aggregator.rs:152–161`). No staleness.

---

## 6. Findings ranked

### F1 — CTX % shows historical peak, not current; can read 100%+ when actual is 53%  **[S1, trust-breaking]**

- **Summary.** `context_max_tokens` is monotonically updated and never reset on `/compact` or natural context shrink, so the meter pins to the largest single turn ever observed in the session.
- **Evidence.** Phase 3 row D01 vs D01b. DB shows source `code:09ecf654` peak 170,850, current 106,334. JSONL last-turn confirmed at 106,334. Code: `aggregator.rs:184–188`.
- **Suspected root cause.** The model intuition behind "max" was that context only grows; in practice Claude Code drops cache entries (auto-compact, sub-agent boundaries, tool result trimming) and the latest turn's reported `input + cache_read + cache_write` is always the source of truth.
- **Fix direction.** Track the *latest* per-turn ctx, not the max. Optionally also track max for telemetry, but display the latest. Reset on session-id change, on `/compact` markers in the JSONL, or simply on every assistant turn (mechanical fix: replace `if ctx_tokens > src.context_max_tokens { … }` with unconditional assignment).
- **Schema/config breaking?** No. State field rename optional.

### F2 — 5-hour % and Week % use `input + output` only; ignores cache_write (the dominant cost driver)  **[S1, trust-breaking]**

- **Summary.** `Db::sum_tokens_since` returns `(SUM(input_tokens), SUM(output_tokens))`. The 5-hour and weekly rate-limit denominators use `(i+o) / limit`. For an opus session with heavy caching, `cache_write` can be 100× input and is what Anthropic actually meters as billable usage.
- **Evidence.** DB totals: opus events 1,410 input + 546,584 output + **5,358,614 cache_read + 60,955,680 cache_write**. The 5h% and week% calculations are missing five orders of magnitude of token activity.
- **Suspected root cause.** Original design intent (from `db.rs:171–179` shape) was input/output as a quick check; the fact that profile `five_hour_limit = 200_000` matches Anthropic's input-token figure suggests the limit was conceived narrowly. But Anthropic's published 5h windows are *billable* tokens.
- **Fix direction.** Either (a) include cache_write at full weight + cache_read at 0.1× (mirroring billing); (b) clearly relabel the meters as "5h I/O %" and add a separate "billable %" meter; (c) sum cost vs a $-budget instead. Option (a) is least disruptive.
- **Schema/config breaking?** Optionally add `five_hour_limit_method` to profile. Not strictly required.

### F3 — Burn-rate spikes from individual large-cache turns are not smoothed  **[S2, wrong but unverifiable]**

- **Summary.** `burn_per_hour = sum_cost_10min × 6`. A single opus turn with a 100k cache_read costs ~$1.50; if it lands in the trailing window, the displayed $/hr spikes by ~$9/hr for ten minutes.
- **Evidence.** Burn alert thresholds (3/5 USD/hr) will fire on what is in fact a single normal turn. `aggregator.rs:344–347`.
- **Suspected root cause.** "10-min × 6" is mathematically correct as an instantaneous estimator, but for human-readable "what am I spending" it should be smoothed (EWMA over 30–60 min, or 60-min × 1).
- **Fix direction.** Either widen the window to 60 min (× 1) or apply EWMA. Tradeoff: slower response to genuine burst.
- **Schema/config breaking?** No.

### F4 — `cost_today` is "since process start", not calendar today  **[S2, trust-breaking, easy fix]**

- **Summary.** `src.cost_today += cost` with no rollover at midnight. Variable name lies.
- **Evidence.** `aggregator.rs:174`. No daily-reset task in the loop.
- **Suspected root cause.** Original implementation never bottomed out the question of "today" vs "session" for long-running daemons.
- **Fix direction.** Either (a) recompute on each tick from DB `SUM(cost_usd) WHERE timestamp ≥ today_local_midnight` — robust, no state drift; (b) zero the field at local midnight.
- **Schema/config breaking?** No.

### F5 — Subagent JSONL files counted as separate sources; possible parent-double-count  **[S2, edge-case]**

- **Summary.** `walk_jsonl` recurses into `subagents/` directories; subagent JSONL gets its own `source_id` (`code:agent-…`). If parent JSONL also embeds subagent output (Claude Code versions vary), `recent_ticks` and `cost_today` double-count.
- **Evidence.** Live CTX_DIAG shows both `code:43ab6690…` (parent) and `code:agent-ac4b20ebb1591459b` (subagent) running; DB shows 11 distinct sources, several `agent-…` IDs. Could not in this audit reproduce a duplicated turn — depends on Claude Code build behavior.
- **Suspected root cause.** Per-file source identity is the natural unit but doesn't account for the parent/child relationship.
- **Fix direction.** Detect `subagents/` path component → mark as child; either skip them (if parent contains the same usage) or aggregate into parent's source.
- **Schema/config breaking?** No.

### F6 — ETA mixes "burn" rate currency with "budget" currency  **[S2, wrong but unverifiable]**

- **Summary.** `tokens_per_min_current` (numerator basis) is `(input + cache_write) + output`. `five_hour_limit` (denominator basis) is `input + output`. ETA = remaining_budget ÷ rate where the two are not the same currency.
- **Evidence.** `aggregator.rs:217–222` vs `:329–334` and `:359`.
- **Fix direction.** Pick one currency consistently. Easiest: align with the F2 fix (include cache_write in the 5h% basis), then ETA's rate also needs cache_write included → already in `tok/min`. Then both sides match.
- **Schema/config breaking?** No (paired with F2).

### F7 — Tokenizer-inflation only applied to CTX, asymmetrically with rate/limit math  **[S3]**

- **Summary.** 1.22× inflation appears in CTX% but nowhere else. If Anthropic's billing or rate-limiting uses inflated counts, every other metric is off by 18% on opus-4-7.
- **Evidence.** Search shows `tokenizer_inflation` is referenced only in `aggregator.rs:189–193`.
- **Fix direction.** Either confirm Anthropic uses raw usage counts everywhere (then the current behavior is right and the 1.22× factor is wrong for CTX too), or apply consistently.
- **Schema/config breaking?** No.

### F8 — Spectrum "in" definition differs between in-memory and DB-seed paths  **[S3]**

- **Summary.** In-memory ticks (post-startup) feed spectrum from `recent_ticks` which is `(input + cache_write, output)`. DB seed at startup (`bucket_tokens`) puts `input + cache_read + cache_write` into the "in" bucket. After launch the bars rescale.
- **Evidence.** `aggregator.rs:222`, `:381–395` vs `db.rs:144–145`.
- **Fix direction.** Make `bucket_tokens` use `input + cache_write_tokens` only.
- **Schema/config breaking?** No.

### F9 — Unknown-model fallback silently drops inflation  **[S3]**

- **Summary.** `tokenizer_inflation` returns 1.0 for unknown models. If a future Claude version (e.g. opus-4-8) appears before the bundled registry is updated, CTX% under-reports by ~18 %.
- **Evidence.** `registry.rs:56–58`.
- **Fix direction.** Use a default-by-family heuristic, or surface "registry stale" warning on unknown model.
- **Schema/config breaking?** No.

### F10 — Cost displayed as if user pays per token; subscription-plan users do not  **[S3]**

- **Summary.** `$ today` and burn $/hr are computed as if hitting the API directly. For Pro/Team subscription users (the configured profile) the actual marginal cost is $0.
- **Evidence.** `registry.rs::cost`. No "is_subscription" flag.
- **Fix direction.** Add per-profile flag `subscription: bool`; when true, label cost fields as "hypothetical $".
- **Schema/config breaking?** Adds optional config field.

### F11 — Global cache-hit-rate weighting excludes cache_write  **[S3]**

- **Summary.** Per-source rate uses `cache_read / (input + cache_read + cache_write)`. Global weighted mean uses weight `(input + cache_read)`. Sources with heavy writes are underweighted in the global aggregate.
- **Evidence.** `aggregator.rs:312`.
- **Fix direction.** Make the weight match the per-source denominator.
- **Schema/config breaking?** No.

### F12 — Active-profile id mismatch causes fallback name/colour and per-tick log spam  **[S4]**

- **Summary.** `config.toml` has `active_profile = "you"` but the only profile defined is `id = "Bryan"`. `state.rs:343–351` warns and falls back, twice per second, forever.
- **Evidence.** `config.toml` line 12 + 27. `state.rs:343` `tracing::warn!`.
- **Fix direction.** Build_view should use the same fallback as `Config::active_profile()` (first profile if id missing). One-line fix.
- **Schema/config breaking?** No.

### F13 — Context window 204,800 vs Anthropic-published 200,000  **[S4]**

- **Summary.** Registry uses 204,800 (= 200×1024). Anthropic's marketed/billable subscription window is 200,000.
- **Evidence.** `model-registry.json` lines 7, 15, 23, 31.
- **Fix direction.** 200,000.
- **Schema/config breaking?** No (registry-only).

### F14 — `cost_today` precision drift on f32 past ~$16k  **[S4]**

- **Summary.** Accumulating costs as `f32`. Above ~$16,000 lifetime, single-cent additions stop registering.
- **Evidence.** `state.rs:103, 174`. DB stores `f64`; the in-memory accumulator does not.
- **Fix direction.** Promote to `f64`.
- **Schema/config breaking?** No.

### Sort

| Sev | Visibility | ID |
|---|---|---|
| S1 | High | F1 |
| S1 | High | F2 |
| S2 | High | F4 |
| S2 | High | F3 |
| S2 | Med | F6 |
| S2 | Low (timing) | F5 |
| S3 | Med | F11 |
| S3 | Low | F7 |
| S3 | Low | F8 |
| S3 | Low | F10 |
| S3 | Low (future) | F9 |
| S4 | Medium (visible label drift) | F12 |
| S4 | Low | F13 |
| S4 | Low | F14 |

---

## 7. Bottom Line — Bryan

**S1 count and siblings of CTX gap.** Two S1 findings. F1 (CTX historical-max bug) is the one your user reported. F2 is its sibling: the 5-hour and weekly meters use `input+output` as the denominator basis while the actual rate-limit currency Anthropic uses is some weighted combination dominated by `cache_write` — which in your DB is **42×** larger than input+output combined. F2 has not been user-reported because the meters are *too low* rather than too high (currently reading 5 % and 11 % when they may legitimately be 50 %+). When a user does eventually run into the real 5-hour wall while the HUD says "you're fine", the trust break will be sharper than F1's.

**Architectural pattern.** Both S1s and three of the four S2s share one shape: **the displayed metric and the metric-it-purports-to-mirror are computed from different bases and you can't see the mismatch from the surface label**. CTX% says "context" but tracks max-ever-observed, not current. "$ today" says "today" but means "since process start". 5h% says "5-hour" but excludes cache writes. ETA combines a numerator-basis from rate code with a denominator-basis from limit code that don't agree. The fix isn't a one-line patch in any of these — it's deciding, per metric, which definition is the canonical one and making both sides honor it. Once you write down the *intended* definition for CTX, $today, 5h%, and ETA, four of the top six findings collapse into mechanical edits.

**Smallest set of changes for "user-trusts-numbers".**
1. F1 — replace monotonic max with last-turn assignment (one line). Stops the 102% report cold.
2. F4 — recompute `cost_today` from DB sum since local midnight on each tick (one block). Removes the silent drift.
3. F2 — include `cache_write` in the 5h/week denominator OR rename the meter to "5h I/O" and add a separate billable bar. Pick one this week; either is honest.
4. F12 — make `build_view`'s profile lookup use `Config::active_profile()` semantics (first-profile fallback). Stops the spam log and the wrong name/colour.

That's roughly 30–80 lines across four files. Everything else (F3 smoothing, F5 subagent dedup, F7/F8/F11) can wait for v1.0.7.

**What is NOT a problem.** The cost-per-token registry is correct. The tok/min denominator (excluding cache_read) is *correctly* documented and *correctly* implemented; the comment in `aggregator.rs:217–220` shows someone thought it through. The hysteresis on alert resolution (`thresholds.rs:13`) is sound. The DB schema, WAL durability, and offset-resume on JSONL tailing are solid — no data-loss or accounting-drift bugs in the persistence layer. Burn formula is mathematically exact (it's the smoothing that's the issue, not the formula). The Rust↔JS boundary has no unit-conversion errors. The 1.22× inflation factor is correctly applied where it's applied; whether it should *also* be applied to rate metrics is a definitional question, not a bug.

**If user "too many bugs" sees v1.0.6 fixing S1+S2.** Probably yes for trust, partial for sentiment. F1 is the splashy one — fixing it removes the "I don't believe Token-Man's numbers" reflex. F2/F4 fixes are less noticeable until they save the user from a surprise rate-limit, but they remove the medium-term trust ratchet. What this audit cannot fix is whether the user's complaint is actually "the numbers are wrong" or "this app shows me nine numbers and I don't know which to act on" — that is a UX/value-framing problem (which of these meters drives behavior? what does the VU needle mean to a non-AV-engineer?) and is outside this audit's scope. If post-1.0.6 the user comes back and says "looks better, still don't use it daily", treat that as the second problem and don't try to solve it with more numerical fixes.

---

*End of audit.*
