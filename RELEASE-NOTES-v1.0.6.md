# Token-Man v1.0.6 — Release Notes

## What changed visibly

When you launch v1.0.6 against your existing `state.db`:

- **CTX %** will drop. Sessions that had compacted or naturally shed
  context were previously pinned to their historical peak. Concrete
  example from your live DB at audit time: source `09ecf654…` previously
  read **101.78 %**; the most recent turn's true fill is **73.11 %** (and
  will track `/context` going forward, not the all-time peak).
- **5h %** and **Week %** will rise — possibly dramatically. From your
  live DB: 5h was **5.17 %**, post-fix it is **529.66 %** (the window has
  in fact been exceeded for a while; the meter will clamp at 100 % for
  display but the raw label will read the true number). Week was
  **11.05 %**, now **1407 %**. These higher numbers are not new usage —
  they are the same usage finally being counted.
- **$ today** will reset to today's actual spend. From your live DB:
  was reading **$234.01** (process-lifetime), now **$4.60** (today only,
  local-midnight bounded).
- **Burn $/hr** will be smoother. A single $3.25 cache turn used to pin
  burn at $19.50/hr for ten minutes; now it ramps in and out with a
  ~25-second time constant. The underlying 10-min-window definition is
  unchanged — only the display is smoothed.
- **Profile name** will read **"Bryan"** instead of **"you"**. The
  previous behavior was a config-mismatch fallback that also produced
  ~2 log warnings per second; both are fixed.

## What this means

The numbers on v1.0.5's HUD were wrong in ways that shifted user trust
the wrong direction. CTX% and $ today were inflated (or, more precisely,
sticky). 5h% and Week% were under-reported, which is the more dangerous
direction — a HUD that shows "5 %" when you're actually past the cap
will get you rate-limited with no warning. v1.0.6 corrects all five.

This release is **correctness only**. There are no new metrics, no new
displays, and no schema changes. If you've been building habits around
the numbers Token-Man showed in v1.0.5, plan on those habits needing
recalibration once v1.0.6 starts reporting the right values.

Two things are intentionally **not** in this release:

- **Display-layer audit (v1.0.7).** The audit ran read-only because the
  v1.0.5 binary has no `--headless` flag. Phase 3 worked from
  CTX_DIAG live log + DB-row derivation + JSONL tail. v1.0.7 will add a
  headless mode and a fuller display-layer audit pass — the kind that
  asserts "what's on the screen" rather than "what the code would
  compute".
- **Tokenizer 1.22× inflation factor (v1.1).** The current 1.22×
  multiplier on opus-4-7 CTX is empirically tuned to make CTX% match
  `/context`. It's a workaround for not having Anthropic's true
  tokenizer. v1.1 will integrate a real tokenizer and a dedicated spec
  for how inflation interacts with all rate metrics, not just CTX.

See `V1.0.6-VERIFICATION.md` for before/after numbers per fix and
`AUDIT-v1.0.5-FINDINGS.md` for the underlying analysis.
