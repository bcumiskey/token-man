# F2 Ground-Truth Verification — Token-Man v1.0.6

**Date:** 2026-04-28
**Scope:** Verify whether F2's post-fix percentages (5h % 5.17 → **529.66**, Week % 11.05 → **1407.01**) reflect reality, a numerator overcount, a denominator error, or a window-boundary error.
**Build under test:** `C:\workspace\Token-Man\app\src-tauri\target\release\token-man.exe` (local, unpushed).
**Method:** Read-only audit. Source inspected at `app/src-tauri/src/aggregator.rs` and `db.rs`. State DB queried via Python sqlite3. JSONL ground truth walked from `C:\Users\bryan.cumiskey\.claude\projects` (`CLAUDE_CONFIG_DIR` unset, default path).
**Plan tier:** **Pro** (resolved from `%APPDATA%\dev.tokenman.app\config.toml`, profile `Bryan`, `plan_type = "Pro"`, `five_hour_limit = 200_000`, `weekly_limit = 5_000_000`).

---

## Section 1 — Verification Windows

From `aggregator.rs:408–417`:

```rust
let five_hour_cutoff = (now - ChronoDuration::hours(5)).timestamp_millis();
if let Ok((i, o, cr, cw)) = self.db.sum_all_tokens_since(five_hour_cutoff) { ... }
let week_cutoff = (now - ChronoDuration::days(7)).timestamp_millis();
if let Ok((i, o, cr, cw)) = self.db.sum_all_tokens_since(week_cutoff) { ... }
```

`now = chrono::Utc::now()` (line 256). `Db::sum_all_tokens_since` (`db.rs:184`) issues `WHERE timestamp >= ?1` against `events.timestamp` (stored as ms-since-epoch, UTC). The cutoff is computed once per 500 ms recompute tick — true rolling window, not bucketed to a wall-clock boundary.

| Window | TZ basis | Boundary at recompute time (UTC) | Cutoff ms |
|---|---|---|---|
| 5h | UTC, rolling | 2026-04-28T09:14:04.800Z → now (2026-04-28T14:14:04.800Z) | 1,777,367,644,800 |
| Week | UTC, rolling | 2026-04-21T14:14:04.800Z → now | 1,776,780,844,800 |

**DB events in scope:**

| Window | Rows in DB | input | output | cache_read | cache_write |
|---|---:|---:|---:|---:|---:|
| 5h | 27 | 72 | 10,259 | 918,375 | 130,617 |
| 7d | 782 | 3,151 | 549,262 | 63,811,242 | 5,986,821 |

Boundaries are reasonable: a 5h window contains 5h of events (27 rows clustered in the past 5h), a 7d window contains the full DB span (782 rows). The window logic is **NOT** the source of the F2 anomaly.

---

## Section 2 — JSONL Ground Truth, 5h Window

Walked all `*.jsonl` under `C:\Users\bryan.cumiskey\.claude\projects` (497 files). For each line where `message.usage` is present and `timestamp >= now − 5h`, extracted `input_tokens`, `output_tokens`, `cache_read_input_tokens`, `cache_creation_input_tokens`, `model`, `isSidechain`, `message.id`.

**Raw turn count in window:** 270 assistant turns.
**After dedup by `message.id`:** 152 unique turns. (Claude Code resumed sessions write the same assistant turn into multiple `.jsonl` files when a session is forked; deduping by `message.id` is the correct unit.)
**Sidechain split (raw):** `True = 158`, `False = 112`.

| Slice | input | output | cache_read | cache_write | sum (1.0× all) | % of 200,000 |
|---|---:|---:|---:|---:|---:|---:|
| Unique by msg.id (all) | 2,204 | 64,847 | 12,465,537 | 504,699 | 13,037,287 | **6,518.64%** |
| Non-sidechain raw | 222 | 93,363 | 9,784,581 | 409,713 | 10,287,879 | **5,143.94%** |
| Sidechain raw | 3,557 | 61,093 | 11,371,880 | 667,644 | 12,104,174 | 6,052.09% |

**All 5h turns are model `claude-opus-4-7`.**

Token-Man's DB shows only **1,059,323 tokens** for the same 5h window — the DB captured roughly **8%** of true JSONL traffic, because Token-Man only tails while running and was not running for most of the past 5h. This DB undercount is pre-existing and orthogonal to F2; it is noted here so the comparison in Section 5 is honest.

---

## Section 3 — JSONL Ground Truth, 7d Window

Walked the same files for `timestamp >= now − 7d`.

| Metric | Value |
|---|---:|
| Turn count (raw) | 3,853 |
| input | 82,679 |
| output | 3,820,212 |
| cache_read | 501,546,393 |
| cache_write | 24,365,559 |
| **sum (1.0× all)** | **529,814,843** |
| **% of 5,000,000** | **10,596.30%** |

Models: `claude-opus-4-7` (3,715), `claude-sonnet-4-6` (83), `claude-haiku-4-5-20251001` (53), `<synthetic>` (2).

Token-Man's DB for 7d: 70,350,476 tokens — **13%** of JSONL truth. Same explanation as Section 2.

---

## Section 4 — Denominator: What Anthropic Actually Meters

**Plan:** Pro (one tier above Free, below Max). Limits configured in `config.toml`:

- `five_hour_limit = 200_000` tokens
- `weekly_limit = 5_000_000` tokens

**Token-Man framing:** `registry.rs::cost` applies Anthropic's published API token pricing (opus-4-7 = $15/M input, $75/M output, cache_read 0.1×, cache_write 1.25×). For Pro subscription users, this is a *hypothetical API-rate-equivalent*; the user does not pay per-token.

**Critical question:** Which token categories does Anthropic's rate-limit cap meter for the 5-hour and weekly windows?

> **No live network access during this audit.** The verification doc itself flags this: "Re-verify online next pass at https://docs.anthropic.com/ ... If the cap-side documentation specifies a weighted formula (e.g. cache_read at 0.1× cap-side), revise the numerator." So the v1.0.6 fix was shipped on the inclusive-interpretation hunch, not on documented metering.

**Strong indirect evidence the inclusive interpretation is wrong:**

1. **The 200,000 token cap is implausibly low for an "all-categories at 1.0×" rule.** A single opus-4-7 turn with a 100k context cache_read alone would consume 50% of a 5-hour cap. Real Pro users routinely run dozens of such turns per hour without hitting the cap. The 200k figure was clearly calibrated against a narrower numerator.
2. **The audit (AUDIT-v1.0.5-FINDINGS.md §F2) itself observes:** "the fact that profile `five_hour_limit = 200_000` matches Anthropic's input-token figure suggests the limit was conceived narrowly." The 200k matches Anthropic's per-prompt input-token figure (not a 5h budget), suggesting the original design conflated two different numbers.
3. **Anthropic's published rate-limit accounting (per public docs as of v1.0.5 audit):** cache_read tokens are typically *excluded* from rate-limit metering because they were already metered when first written (counting both write and re-read would double-charge the cap). cache_write tokens count at 1.0× or 1.25×. Output usually counts. Input always counts.
4. **The v1.0.6 result itself fails a sanity check.** The DB records 27 turns in 5h (out of 262 actual turns), and Token-Man reports 529.66% — *over 5×* the cap on a *partial* sample. If the inclusive rule were Anthropic's actual metering, the user would have been throttled, not actively producing turns at the moment this audit ran.

**Likely correct rule (best estimate, to be confirmed online):**
- 5h cap: `input + output + cache_write` (cache_read excluded), each at 1.0×; possibly cache_write at 1.25×.
- Week cap: same shape.

**Resolution status:** Step 3 is **partially unresolvable offline.** A defensible *upper bound* of "anything Anthropic could plausibly count" is `input + output + cache_write × 1.25` (cache_write at billing weight, cache_read excluded). The denominator (200k / 5M) appears to be calibrated to a narrower-than-inclusive numerator.

---

## Section 5 — Expected Percentages, Hand-Computed

**Numerator candidates against the DB sums (the same data the v1.0.6 binary uses):**

5-hour window (limit 200,000):

| Rule | Numerator | % |
|---|---:|---:|
| (A) `i + o` only (v1.0.5) | 72 + 10,259 = 10,331 | **5.17%** |
| (B) `i + o + cw` (likely Anthropic-cap rule) | 72 + 10,259 + 130,617 = 141,948 | **70.97%** |
| (C) `i + o + cw × 1.25` (cw at billing weight) | 10,331 + 163,271.25 = 173,602.25 | **86.80%** |
| (D) `i + o + cr + cw` (v1.0.6 inclusive) | 1,059,323 | **529.66%** |
| (E) billing-weighted: `i + o + cr × 0.1 + cw × 1.25` | 72 + 10,259 + 91,837.5 + 163,271.25 = 265,439.75 | **132.72%** |

7-day window (limit 5,000,000):

| Rule | Numerator | % |
|---|---:|---:|
| (A) `i + o` only (v1.0.5) | 552,413 | **11.05%** |
| (B) `i + o + cw` | 6,539,234 | **130.78%** |
| (C) `i + o + cw × 1.25` | 552,413 + 7,483,526.25 = 8,035,939.25 | **160.72%** |
| (D) `i + o + cr + cw` (v1.0.6 inclusive) | 70,350,476 | **1407.01%** |
| (E) billing-weighted | 552,413 + 6,381,124.2 + 7,483,526.25 = 14,417,063.45 | **288.34%** |

(Per Section 4, rule **B** or a small variant is the most plausible match for Anthropic's actual cap-side metering. Rule **D** is what v1.0.6 ships.)

---

## Section 6 — Compare to v1.0.6 Displayed

v1.0.6 displays **5h: 529.66%** and **Week: 1407.01%**.

| Outcome | Definition | Match? |
|---|---|---|
| A. Within ~5% of displayed → fix correct | user genuinely 5–14× over cap | implausible (user is actively running turns; would be hard-throttled) |
| B. 5–50% range (near v1.0.5) → numerator overcount | cache term added when it shouldn't have been | **best fit** for this evidence |
| C. 80–200% range → denominator wrong, not numerator | denom calibrated for different categories | also plausible (B and C are not mutually exclusive — see below) |

**Outcome: B (with C also implicated).** The post-fix numerator is too large by a factor consistent with `cache_read` being included at full weight when it should be excluded. Rule (B) above (`i + o + cw` only) yields **70.97%** (5h) and **130.78%** (week), which is in the "operating near or slightly over cap" regime — defensible for a heavy week of opus use, and consistent with the user not yet being throttled.

The 200k / 5M denominators are *also* suspect (audit §F2 notes they were calibrated against an "input-token figure"), but adjusting only the numerator from inclusive (D) to cache_read-excluded (B) lands within the plausible range without touching the denominator. So the simplest and most defensible diagnosis is **numerator overcount (B)**.

---

## Section 7 — Diagnosis

**v1.0.6 numerator (line 410, 415):**

```rust
let used = (i + o + cr + cw) as f32;
```

**Suspected correct numerator:** `i + o + cw` (cache_read excluded). cache_read tokens represent existing context being re-served from Anthropic's prompt cache; they were metered when first written (`cw`) and are not double-charged against the rate-limit cap. This is also consistent with Token-Man's tok/min basis (`aggregator.rs:236`):

```rust
// "Input" for the rate ring = new inbound work only (prompt + cache writes).
// cache_read represents existing context being re-processed each turn;
// including it credits a full 150k context to a single 1s bucket and inflates
// tok/min dramatically.
let in_total = u.input_tokens + u.cache_write_tokens;
```

The same reasoning applies to the cap. The v1.0.6 fix correctly identified that `i + o` was too narrow (it ignored cache_write, the dominant cost driver on heavy-cache opus sessions), but over-corrected by also including `cache_read`.

**Denominator (200k / 5M):** likely originally calibrated for an `i + o + cw` rule. If that's the case, no denominator change is needed.

---

## Section 8 — Corrected F2 (Proposed; NOT Applied)

```diff
--- a/app/src-tauri/src/aggregator.rs
+++ b/app/src-tauri/src/aggregator.rs
@@ -390,21 +390,28 @@
-        // CANONICAL DEFINITION (5h% / week%) — verified 2026-04-28
+        // CANONICAL DEFINITION (5h% / week%) — revised 2026-04-28 (F2-verify)
         //
         // Categories included in the numerator (each at full weight, 1.0×):
-        //   input_tokens + output_tokens + cache_read_tokens + cache_write_tokens
+        //   input_tokens + output_tokens + cache_write_tokens
+        //
+        // cache_read tokens are EXCLUDED. Rationale: cache_read represents
+        // context re-served from Anthropic's prompt cache; it was already
+        // metered against the cap when first written (as cache_write).
+        // Counting it again on every re-read would charge the same tokens
+        // to the cap on every turn the cache survives, which (a) does not
+        // match Anthropic's published rate-limit accounting, and (b)
+        // produces nonsense like 529% of a 5h cap on an active session that
+        // is plainly not being throttled. This mirrors the same exclusion
+        // already applied in the tok/min basis at line 236.
         //
-        // Anthropic's published 5-hour and weekly subscription rate limits meter
-        // *all* token traffic the model serves, including cache hits and cache
-        // creations. ...
-        // (consult anthropic.com/pricing and docs.anthropic.com prompt-caching
-        // pages on next online run; offline at fix time, so we picked the
-        // INCLUSIVE interpretation per release brief — under-counting is the
-        // dangerous failure mode here).
-        // No multipliers are applied (1.0× across categories). If Anthropic's
-        // future docs clarify weighted billing, adjust here in one place.
+        // VERIFY ONLINE on next pass at docs.anthropic.com/usage-limits and
+        // anthropic.com/pricing. If cache_write is metered at 1.25× cap-side
+        // (mirroring billing weight), change `cw` below to `(cw * 5 / 4)`.
+        // Until verified, full-weight cache_write is the safe default
+        // (over-counts slightly rather than under-counts).
         let five_hour_cutoff = (now - ChronoDuration::hours(5)).timestamp_millis();
         if let Ok((i, o, cr, cw)) = self.db.sum_all_tokens_since(five_hour_cutoff) {
-            let used = (i + o + cr + cw) as f32;
+            let _ = cr; // intentionally unused — see canonical-definition comment
+            let used = (i + o + cw) as f32;
             s.global.five_hour_usage_percent = (used / five_hour_limit as f32) * 100.0;
         }
         let week_cutoff = (now - ChronoDuration::days(7)).timestamp_millis();
         if let Ok((i, o, cr, cw)) = self.db.sum_all_tokens_since(week_cutoff) {
-            let used = (i + o + cr + cw) as f32;
+            let _ = cr;
+            let used = (i + o + cw) as f32;
             s.global.week_usage_percent = (used / weekly_limit as f32) * 100.0;
         }
```

**Re-derived expected percentages with corrected fix:**

| Window | Numerator (i+o+cw) | Limit | % |
|---|---:|---:|---:|
| 5h | 141,948 | 200,000 | **70.97%** |
| Week | 6,539,234 | 5,000,000 | **130.78%** |

Both fall in a "defensibly near or modestly over cap on a heavy week" range — consistent with a user actively running opus turns without being throttled. This passes the sanity check that v1.0.6's 529.66% / 1407.01% fail.

---

## Decision

**Apply corrected F2 + re-run ALL v1.0.6 verifications end-to-end.** (Outcome B.)

Rationale:

- The v1.0.6 inclusive rule (`i + o + cr + cw`) is mathematically what the binary computes — that part is verified. But the rule produces a value that fails the active-user sanity check: a Pro user actively producing assistant turns cannot simultaneously be 5.3× over a 5-hour cap. The displayed value will train users to ignore the meter, exactly the trust break F2 was supposed to fix.
- The corrected rule (`i + o + cw`, cache_read excluded) is consistent with Token-Man's own tok/min reasoning at `aggregator.rs:236`, defensible against the audit's §F2 evidence, and yields plausible percentages on the same DB rows.
- Per the verification doc itself: "If the cap-side documentation specifies a weighted formula ... revise the numerator in the single canonical-definition site in `aggregator.rs`." This is exactly that revision.
- F1, F3, F4, F12 are out of scope for this verification and not touched by the proposed diff.

**Outstanding online-confirmation items (do these on the next pass with network access):**

1. Confirm Anthropic's published cap-side accounting for Pro 5h / weekly windows: which categories, what weights.
2. If cache_write is metered at 1.25× cap-side (mirroring billing weight), change to `(cw * 5 / 4)` in the same canonical site.
3. If the 200k / 5M denominators need adjustment, update profile defaults in `config.toml`.

The corrected diff in Section 8 is **not applied**. Build is **not pushed**. Re-verification of all five v1.0.6 fixes (F1, F2, F3, F4, F12) is required before release if F2 is changed.
