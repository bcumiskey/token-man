# Token-Man — v1.0 app scaffold

> It really whips the token's ass.

Tauri 2 + Rust + vanilla JS implementation of the HUD specified in
`../Phase2/PHASE-2-SPEC.md`. Scaffolded end-to-end: backend modules, frontend
port of the mockup, settings window, IPC wiring, SQLite state store, threshold
engine, and all three data sources.

## Known Limitations

- Session actions (focus, /compact, /clear) are not included in this release.
  Planned for v1.1 — requires cross-process window focus and stdin injection.
- Cost display on subscription accounts: Token-Man's TODAY cost figure is
  hypothetical — it shows what your usage would cost at API rates. Claude
  Max/Pro subscription users are not actually being charged per-token;
  they're on a flat-rate subscription. The cost display is useful as a
  relative indicator of consumption but does not reflect real out-of-pocket
  spend. A proper subscription-aware cost display is planned for v1.1.

## Status

- `cargo check --all-targets` — clean (0 warnings, 0 errors)
- `cargo build` (debug) — links successfully to `target/debug/token-man.exe`
- Release bundle (`tauri build`) and installer signing — not yet run; requires
  `@tauri-apps/cli` + a code-signing cert.

## Layout

```
app/
├── package.json                  # node-side glue for @tauri-apps/cli
├── src/                          # frontend (served as static files by Tauri)
│   ├── index.html                # HUD, ported from the mockup
│   ├── settings.html             # settings modal (tabbed)
│   ├── styles.css
│   ├── main.js                   # state-update subscriber + DOM render loop
│   ├── settings.js
│   └── assets/                   # SVG mascots
└── src-tauri/
    ├── Cargo.toml
    ├── tauri.conf.json
    ├── capabilities/default.json
    ├── icons/                    # generated placeholders
    ├── assets/
    │   ├── model-registry.json   # bundled; refreshable at runtime
    │   ├── jingle.wav            # synthesized placeholder — commission real audio
    │   └── alert.wav             # synthesized placeholder
    └── src/
        ├── main.rs               # thin entry
        ├── lib.rs                # Tauri builder, spawns workers + render tick
        ├── paths.rs              # app-data / config / db paths
        ├── config.rs             # TOML load/save + Profile, Thresholds
        ├── db.rs                 # SQLite schema + event / rollup / offset tables
        ├── state.rs              # AppState, SourceState, view types
        ├── registry.rs           # ModelRegistry (pricing, context windows)
        ├── aggregator.rs         # consumes events, recomputes metrics, fires alerts
        ├── thresholds.rs         # warn/critical evaluation + global-state promotion
        ├── ipc.rs                # #[tauri::command] handlers
        ├── snapshot.rs           # Markdown snapshot for clipboard
        ├── audio.rs              # rodio playback
        ├── tray.rs               # system tray + menu
        └── sources/
            ├── mod.rs            # SourceEvent enum
            ├── ccusage.rs        # JSONL tailer (notify + 2s safety poll)
            ├── otel.rs           # local OTLP/HTTP receiver on 4318
            └── admin.rs          # Admin API poller with keyring-backed secrets
```

## Running

```bash
# one-time
cd app && npm install

# development (hot reload frontend + rebuild Rust on change)
npx tauri dev

# release build (creates installer under src-tauri/target/release/bundle)
npx tauri build
```

If you only want to run the binary without the Tauri CLI:

```bash
cargo run --manifest-path src-tauri/Cargo.toml --release
```

## What's wired

- **ccusage JSONL tailer** (`sources/ccusage.rs`) — watches
  `~/.claude/projects/**/*.jsonl`, streams per-turn usage events, parses
  `input_tokens` / `output_tokens` / `cache_creation_input_tokens` /
  `cache_read_input_tokens`.
- **OTel receiver** (`sources/otel.rs`) — listens on 127.0.0.1:4318, accepts
  `/v1/metrics|traces|logs`. v1.0 drops payloads but the presence signal is
  wired. Point Claude Code at it with
  `CLAUDE_CODE_TELEMETRY_ENDPOINT=http://127.0.0.1:4318`.
- **Admin API poller** (`sources/admin.rs`) — per-profile polling on 60s
  default; API keys live in the OS keychain via the `keyring` crate.
- **Aggregator** (`aggregator.rs`) — consumes all three sources, updates
  in-memory state, persists to SQLite, recomputes metrics every 500ms,
  advances the 60s/1h/24h spectrum buffers on a 1s tick.
- **Threshold engine** (`thresholds.rs`) — promotes global state OK→WARN→ALERT,
  emits toast notifications and alert sounds on transitions, de-dupes sustained
  alerts, auto-resolves when the metric recovers.
- **Settings UI** — General / Profiles / Thresholds / Notifications / Sources /
  About tabs; edits persist to `config.toml` and the keychain.
- **Snapshot-to-clipboard** — Markdown via the SNAP button.
- **Tray + pin/minimize/close** — clicking × hides to tray when
  `minimize_to_tray` is on; tray icon restores.
- **Model registry** — bundled JSON with pricing & context windows; refreshable
  at runtime via the status-bar REG cell.

## Token-efficiency safeguards (§8 of the spec)

Confirmed in code: the app never imports the Anthropic SDK, never calls the
Messages API, and never forwards a user's prompt anywhere. It only reads local
JSONL, receives local OTel, and calls the Admin API (aggregate usage — no
content).

## Known gaps vs. v1.0 ship criteria

- [ ] `tauri build` release bundle + installer signing (needs cert)
- [ ] Real jingle/alert WAV assets (placeholders synthesized in Python)
- [ ] Icons are placeholders, not the Token-Man mascot rendered at 16/32/48/
      128/256
- [ ] OTel receiver accepts and drops; decoding OTLP protobuf deferred
- [ ] Admin API response shape is based on the spec's example; verify against
      the live endpoint at build time
- [ ] Tests — none yet; the JSONL parser in particular deserves a small
      fixture-based suite
- [ ] Session actions (focus/compact/clear) emit advisory events but the app
      does not control target processes — documented behavior for v1.0

Ship when those gaps close. Everything else from §12 is already in place.
