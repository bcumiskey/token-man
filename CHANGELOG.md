# Changelog

## v1.0.6 — 2026-04-28 — Numerical correctness

Five fixes from the v1.0.5 audit (`AUDIT-v1.0.5-FINDINGS.md`). All five target
displayed numbers that were computed against a different basis than their
label implied. No new metrics. No display-layer changes. State-DB schema is
unchanged; one in-memory field rename (`context_max_tokens` →
`context_current_tokens` + `context_peak_tokens`).

- **F1 — CTX %** now reflects the most recent assistant turn's context fill,
  not the lifetime peak. Pre-1.0.6 the meter pinned to the largest single
  turn ever observed and never decreased on `/compact` or natural shrink.
  CTX values will drop on launch for any session that has compacted.
- **F2 — 5h % and Week %** now include `cache_read_tokens` and
  `cache_write_tokens` in the numerator (full weight, 1.0× across all four
  categories). Pre-1.0.6 the denominator was `input + output` only, which
  under-counted heavy-cache opus sessions by up to five orders of magnitude.
  Expect 5h% and Week% to read substantially higher post-upgrade — the prior
  values were not just wrong, they were dangerously low.
- **F3 — Burn $/hr** is now EWMA-smoothed (α = 0.02 over the 500 ms
  recompute tick, τ ≈ 25 s). The underlying 10-min × 6 definition is
  unchanged; only the display value is smoothed. A single large-cache turn
  no longer pins burn at full deflection for a full 10 min.
- **F4 — $ today** is now bounded by local-midnight, derived from DB SUM on
  every tick. Pre-1.0.6 the field accumulated for the lifetime of the
  process. The on-launch value will drop sharply if your prior session had
  been running across multiple days.
- **F12 — Active-profile fallback** now resolves to the first profile when
  `active_profile` doesn't match any profile id, mirroring
  `Config::active_profile()` resolution. The "profile not found" warning
  fires once at startup instead of twice per second forever. The
  "admin poll failed" warning is also rate-limited to once per
  (profile, session). Existing config.toml with `active_profile="you"` and
  profile id `"Bryan"` will now display the Bryan profile correctly.
