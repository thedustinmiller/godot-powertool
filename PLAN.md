# Implementation Plan — `gyrs` addon

**Status:** draft, expected to iterate. Informed by `DESIGN.md` and the experiments in `~/Desktop/Godot-webrtc-demos/{yrs-crdt-demo,experiments}/`. Numbering is hierarchical (`0.1`, `0.2.1`, …) so phases can be split, re-ordered, or inserted without renumbering the rest.

Each phase lists **Goal**, **Scope**, **Deliverables (observable)**, **Tests**, **Exit criteria**, and **Depends on**. Open questions are flagged inline with **OQ**.

## Framing

- **Greenfield, not a port.** `yrs-crdt-demo/` and `experiments/*` are reference only; their throwaway code informs design but does not constrain it. Where a cleaner API or implementation exists, take it — relicense / rewrite freely.
- **Observable deliverables first.** Every phase lands with: something you can run or a signal you can watch, plus automated tests at the tier where the behaviour lives (unit, GUT integration, or E2E).
- **Accessibility is load-bearing.** Integrating cleanly with Godot's AccessKit is a hard requirement (ADA), not a polish item. Every control wrapper and presence overlay must preserve or enhance the accessibility tree of the control it wraps. See Milestone A below.
- **GUI state is structural too.** "Sync GUI state" isn't only text fields and toggles — it includes tabs, folds, tree selection/expansion, split positions, item-list selection, scroll offsets where meaningful. Presence annotates these same structural states (who is on what tab, whose cursor is on which tree row).
- **History is short by default.** Target use case is interactive group apps, not long-lived collaborative word processors. Compaction exists, is loud, is manual; unbounded-history mode is out of scope.

## Project layout (post-rename)

- Addon ships at **`godot/addons/gyrs/`**; `plugin.cfg` name = `gyrs`.
- Rust crate is **`gyrs`** (cdylib) at the workspace root or under `crates/gyrs/`. **OQ:** which. *Lean: `crates/gyrs/` so future support crates have a home.*
- `addons/sample_extension/` and its workspace membership are **deleted in 0.1**. No preservation; the template's role is done.
- Bundled relay reference server moves from `experiments/relay_server/` → `relay/`.

---

## Milestone 0 — Scaffold

### 0.1 — Rename + delete template; empty addon loads
- **Goal:** fresh `gyrs` addon scaffold under `godot/addons/gyrs/`; `sample_extension` removed; a smoke scene loads the extension with zero exposed classes.
- **Scope:**
  - Delete `addons/sample_extension/`, its workspace entry, and the copied-into-`godot/addons/` twin.
  - Add `crates/gyrs/` (cdylib) to workspace; `yrs = "0.25"`, `godot = "0.5.x"` native + pinned git rev for WASM (from exp3).
  - `godot/addons/gyrs/{plugin.cfg, gyrs.gdextension, bin/}`. `.gdextension` declares all target library paths per DESIGN §Build.
  - `xtask build linux` produces the `.so` and copies it into `bin/linux/`.
- **Deliverables:** empty addon that loads.
- **Tests:** CI job — `cargo xtask build linux && tools/godot/godot --headless --path godot scenes/smoke.tscn` exits 0.
- **Exit criteria:** green CI; no references to `sample_extension` remain.

---

## Milestone 1 — Core Rust FFI (greenfield)

### 1.1 — `YDoc` + `YText` + UTF-16 offset helpers
- **Goal:** first user-visible primitive. Text insert/delete from GDScript, `changed` observer deferred to `poll_changes()`, updates deferred to `poll_updates()`.
- **Scope:** `core/offsets.rs`, `godot_api/doc.rs`, `godot_api/text.rs`. Origin tagging (`transact_mut_with(client_id)`) baked in from day one — no later retrofit.
- **Deliverables:** `YDoc.create()`, `YText.insert/delete/get_string/length`, `update_produced` + `changed` signals.
- **Tests:**
  - **Unit (Rust):** offset round-trips for ASCII / CJK / emoji / ZWJ / combining / mixed / empty.
  - **GUT integration:** single-doc CRUD; two-doc bidirectional convergence with conflicting ops.
- **Exit criteria:** GUT suite green; origins visible in observer output.

### 1.2 — `YMap` + `YArray` + `conversions`
- **Goal:** non-text shared types.
- **Scope:** `godot_api/{map,array,conversions}.rs`. Nested `yrs::Out::{YText,YMap,YArray}` returns wrapper instances, not errors. Non-String dict keys → `godot_error!` + no-op.
- **Deliverables:** full DESIGN §YMap and §YArray surfaces.
- **Tests:**
  - **Unit (Rust):** Variant↔Any round-trips for every supported type incl. 2-level nesting; non-String keys fail cleanly.
  - **GUT:** CRUD + observer signals; two-doc convergence with conflicting ops on both.

### 1.3 — Reentrancy guard **[prototype phase]**
- **Goal:** decide the mechanism before committing to an API. DESIGN §Validated constraints names the need; the *shape* of the deferred-call path inside godot-rust is unresolved.
- **Scope:** spike 2–3 approaches against a failing reentrancy test:
  - (a) Godot `Callable` with `CONNECT_DEFERRED` emitted from Rust.
  - (b) Hidden internal autoload Node draining a per-doc queue in `_process`.
  - (c) Queue-only (no deferred call); the next user-driven `poll_*()` drains it.
- **Deliverables:**
  - Reentrancy-spike write-up in `docs/decisions/001-reentrancy.md` with timing numbers, ergonomics notes, and a pick.
  - Chosen approach lands in `core/reentrancy.rs` + wired into `YText`/`YMap`/`YArray`/`YUndoManager` mutators.
- **Tests:**
  - **Unit (Rust):** guard AtomicBool + queue draining + depth cap 1.
  - **GUT:** regression — handler mutates inside `changed`, doc converges, no deadlock, a warning is logged.
- **Exit criteria:** written decision record; guard ships; test red without the guard.
- **Depends on:** 1.2.

### 1.4 — `YUndoManager`
- **Goal:** DESIGN §YUndoManager.
- **Scope:**
  - Auto `include_origin(client_id)` at construct time; `track_all_origins` option overrides.
  - `capture_timeout_ms` (default 500), `reset()`, `clear()`.
  - `stack_changed` signal.
  - Reentrancy guard catches `undo()`/`redo()` calls from observer handlers (defer to next frame).
  - On `peer_left`: transport notifies undo manager to drop frames tagged with that origin.
- **Deliverables:** DESIGN surface in full.
- **Tests:**
  - **GUT:** undo/redo respects origin; remote ops never on local stack; `reset()` boundary correctness; time-grouping at 500 ms edge.
  - **E2E (two peers):** A edits, B edits, A undoes → only A's rolls back.
- **Depends on:** 1.3.

### 1.5 — `YAwareness` (Rust core + wrapper)
- **Goal:** DESIGN §YAwareness with dirty-tracking default.
- **Scope:**
  - `core/awareness.rs`: per-client `{clock, state, last_seen}`, LWW on clock, 30 s prune.
  - Rust-owned `last_sent_state` snapshot; `encode_update()` diffs; `encode_full_update()` ignores diff. Snapshot updates atomically after encode.
  - `FULL_STATE` / `DIRTY_TRACKED` strategies; default dirty.
- **Deliverables:** full DESIGN surface.
- **Tests:**
  - **Unit (Rust):** clock LWW; prune; 3-client merge; dirty-tracking preserves canonical state across N encodes; `FULL_STATE` equivalence.
  - **GUT:** bandwidth sanity — at N=10, 10 Hz, dirty ≤ 3 KB/s/peer (matches exp5C, with margin).

---

## Milestone 2 — GDScript layer (non-structural + non-text)

### 2.1 — `YPoller` autoload
- **Goal:** DESIGN §YPoller; zero-config polling.
- **Scope:** autoload registered by the plugin; `register(WeakRef)` / `unregister`; `_process` walks the registry, calling `poll_*()`.
- **Deliverables:** autoload node, plugin-registered.
- **Tests:**
  - **GUT perf:** N=100 instances mean <200 µs/frame; N=1000 <800 µs. (Headroom on exp5A 78/472 µs.)
  - **GUT correctness:** dead `WeakRef` drops out automatically.
- **Depends on:** 1.5.

### 2.2 — `YBinder` — generic + value-control convenience
- **Goal:** one-liner bindings for the common "map key ↔ widget value" case, plus a generic fallback.
- **Scope:**
  - `bind_property(node, property, y_map, key, signal_name = "", default = null)` — the generic path. Infers change signal from common node types; falls back to polling if none.
  - Convenience wrappers: `bind_checkbox`, `bind_slider`, `bind_line_edit`, `bind_color_picker`, `bind_option_button`, `bind_spin_box`, `bind_range` (covers `HSlider`/`VSlider`/`ProgressBar`/etc.).
  - Serialisation helpers: `Color → #RRGGBBAA`, `Vector2/3 → Array`, `Rect2 → Array`.
  - Local/remote flag dance per DESIGN §The local/remote pattern.
- **Deliverables:** `YBinder` Node with the surface above.
- **Tests:**
  - **GUT integration:** one scene per convenience binder; A flips → B mirrors within one frame.
  - **Property fuzz:** generic `bind_property` against 6 stock controls with different value types.
- **Depends on:** 2.1.
- **OQ:** `bind_button_group` — do we emit the pressed index or the `Button.name`? *Lean: name (stable across reordering).*

### 2.3 — `YTextEdit` **[prototype phase for forward-delete]**
- **Goal:** collaborative `TextEdit` that passes the mechanical IME suite, with a prototype-chosen forward-delete strategy.
- **Scope:**
  - Virtual overrides: `_handle_unicode_input`, `_backspace`, `_cut`, `_copy`, `_paste`, `_paste_primary_clipboard`.
  - Replicate default `_handle_unicode_input` (no `super.*`); honour `caret_index == -1` via `insert_text_at_caret`.
  - Disable built-in undo; set `backspace_deletes_composite_character_enabled = true` at `bind()`.
  - Drive sync from own `YText.changed`; never from `text_changed`.
  - IME idle: gate refreshes on `!has_ime_text()`; `apply_ime()` before forced refresh. Active-compose behaviour is the post-1.0 matrix.
  - **Forward-delete spike:** two branches, decided empirically:
    - (a) `_gui_input` intercepts `KEY_DELETE`, consumes, emits equivalent `YText.delete`.
    - (b) let engine delete; `_process` diff-reconciles against `YText`.
  - Decision record in `docs/decisions/002-forward-delete.md`.
- **Deliverables:** `YTextEdit` class; decision record; reconciler (if b wins) or intercept helper (if a wins).
- **Tests:**
  - **GUT:** port the 41-test mechanical IME suite from `exp6_ime/` under `godot/tests/unit/`.
  - **GUT:** forward-delete cases — single/multi-caret × with/without selection × ZWJ cluster boundary × end-of-line merge.
  - **E2E:** two-editor convergence within 100 ms over local relay.
- **Exit criteria:** IME + forward-delete suites green; decision record merged.
- **Depends on:** 1.3, 2.1.

### 2.4 — `YLineEdit`
- **Goal:** DESIGN §YLineEdit.
- **Scope:** `_gui_input` intercept path (no input virtuals). Expose `text_change_rejected` for max_length-capped binders.
- **Tests:**
  - **GUT:** convergence; max_length overflow observable.
- **Depends on:** 2.3 (shares helpers).

---

## Milestone 3 — Structural GUI wrappers

> New milestone — the "sync GUI state" goal expanded past text. Each wrapper is a thin subclass (or, where the value-property shape is clean, a convenience `YBinder` entry) that mirrors a structural property on a stock control. Presence (Milestone 5) layers per-peer focus/selection on top of these.

For each: scope is the *synced properties* list, deliverables are the wrapper + binder + one test scene, tests are local/remote convergence + AccessKit tree preserved.

### 3.1 — `YTabContainer`
- Sync: `current_tab` (int). *Not* tab labels by default — treat labels as static scene content. Tab-visible state (hide/show per peer) is presence, not sync.
- Tests: A selects tab 2 → B follows; accessibility tree still announces the tab switch.

### 3.2 — `YSplitContainer` (`HSplitContainer`/`VSplitContainer`)
- Sync: `split_offset` (int).
- Debounce: treat drag as a live stream; binder rate-limits to 20 Hz so we don't flood with per-pixel events.

### 3.3 — `YFoldableContainer` (Godot 4.x stock) **OQ: availability**
- Sync: `folded` (bool).
- **OQ:** Godot 4.6 names this `FoldableContainer` in some branches and `ExpandableContainer` in others. Confirm class name at implementation time; fall back to a `bind_property` one-liner if the control is unstable in 4.6 stable.

### 3.4 — `YTree` — structural sync
- Sync: item ordering + expansion state + selection. **Not** item content by default — the app is expected to drive `Tree` items from a `YArray`/`YMap` data source and call `update_tree()` in response to `changed`.
- Two binders:
  - `bind_tree_data(tree, y_array, formatter)` — one-way data → tree population.
  - `bind_tree_ui(tree, y_map)` — two-way expansion + selection state (`collapsed`, `selected_items`).
- This is the most complex wrapper; likely splits into 3.4.1 / 3.4.2.

### 3.5 — `YItemList`
- Sync: selection (indices). Data flow via `YArray` like `Tree`.

### 3.6 — Scroll position (`ScrollContainer`)
- Sync: `scroll_horizontal` / `scroll_vertical` as optional binder (`bind_scroll(sc, y_map, key)`); off by default, since apps often *shouldn't* sync scroll.

### 3.7 — Structural wrapper integration test suite
- One composite scene that mounts every wrapper and asserts A→B convergence + AccessKit tree integrity for each.
- **Depends on:** 3.1–3.6.

---

## Milestone A — AccessKit (cross-cutting, hard requirement)

Landing as a dedicated milestone because it touches every previous milestone and regressions here are legal-exposure. Executed **in parallel with Milestones 2–3** — every control wrapper MUST pass its A-tier tests before its milestone is considered complete.

### A.1 — Baseline audit + pinning
- **Goal:** know what AccessKit in Godot 4.6+ gives us, where it breaks, and our minimum supported version.
- **Scope:**
  - Audit: which `Control` types have complete AccessKit role mapping in Godot 4.6; which are partial.
  - Pin minimum Godot version in `.gdextension` based on what we need.
  - Document target screen readers for CI: NVDA (Windows), Orca (Linux), VoiceOver (macOS), TalkBack (Android), VoiceOver (iOS), platform AT APIs in browsers.
- **Deliverables:** `docs/accessibility/baseline.md`.
- **OQ:** minimum Godot version — 4.6.x suffices for text + common controls; confirm `FoldableContainer`/`Tree` accessibility state.

### A.2 — AccessKit preservation rule for every wrapper
- **Goal:** wrapping a `TextEdit` / `TabContainer` / etc. never *reduces* the accessibility information the stock control provides.
- **Scope:**
  - Every wrapper inherits from the stock control (already planned); ensure no override stomps on `accessibility_*` properties or roles.
  - Where a wrapper alters behaviour (e.g. routing input through a CRDT), ensure the accessibility node value reflects *post-sync* state, not pre-sync.
- **Tests (per wrapper):** GUT test asserts the control's `get_accessibility_*` (or the AccessKit debug tree) produces the expected role, state, and value before and after a remote edit.

### A.3 — Remote-edit announcements (live regions)
- **Goal:** when a remote peer edits, screen readers announce the change appropriately.
- **Scope:**
  - `YTextEdit` / `YLineEdit`: feed remote deltas through Godot's live-region / AT-value-change mechanism (exact API **OQ**: Godot 4.6 calls it `accessibility_live` and `accessibility_announce`; confirm).
  - Configurable verbosity (`Off` / `Polite` / `Assertive`) on the wrapper; default `Polite`.
  - Debounce so a 50-character burst announces once, not 50 times.
- **Tests:**
  - **Unit:** debouncer collapses a 50-insert burst to one announcement.
  - **Manual matrix:** NVDA + Orca + VoiceOver on a two-peer text scene; announcements fire and aren't noisy.

### A.4 — Presence + accessibility
- **Goal:** remote cursors / structural-focus overlays are reachable by AT, not just visual.
- **Scope:**
  - `YRemoteCursors` overlay nodes expose peer name + position as AT-visible labels.
  - On peer join/leave, a polite announcement fires.
  - Structural-focus presence (Alice is on tab 2) surfaces as aria-live-equivalent updates.
- **Tests:** manual matrix; regression GUT confirming overlay nodes have `accessibility_name` set.
- **Depends on:** 5.1.

### A.5 — Accessibility CI smoke
- **Goal:** regressions don't slip through because manual matrix is slow.
- **Scope:** headless Godot scene dumps its AccessKit tree as JSON; snapshot-test against committed fixture. Updates require a PR diff review.
- **Deliverables:** `tools/axtree-dump.gd` + CI job.
- **Tests:** snapshot suite covers every wrapper scene.

---

## Milestone 4 — Transport

### 4.1 — Three-channel topology + peer manager
- **Goal:** channels open, contract defined.
- **Scope:**
  - Rewrite peer manager as `GyrsPeerManager` (no "CRDT" naming in user-facing types where possible; the addon is `gyrs`).
  - Negotiated DataChannels, **three** ids:
    - 1: `doc_sync` — reliable, ordered, binary.
    - 2: `awareness` — unreliable, unordered, binary.
    - 3: `control` — reliable, ordered, binary. Session-meta (compaction, future extensions).
  - Emits `data_channel_message(peer_id, channel_id, data)` + `peer_left(peer_id)`.
- **Deliverables:** manager + signaling client in `godot/addons/gyrs/scripts/transport/`.
- **Tests:**
  - **E2E (GDScript subprocess harness):** two headless Godot instances handshake through local relay; all three channels open.

### 4.2 — Doc sync (tags 0/1/2)
- DESIGN §Doc sync protocol. Handshake on channel open; forward `update_produced` as tag 2.
- **Tests:**
  - **E2E:** A types → B within 100 ms; 10k-char doc syncs under the chunking threshold.

### 4.3 — Chunking (tag 3 on `doc_sync`)
- DESIGN §Large-update chunking. Framing `[3][u32 update_id][u16 chunk_index][u16 total_chunks][bytes…]`; 48 KB threshold; reassembly keyed by `(peer_id, update_id)`; eviction on `peer_left` + 30 s absolute timer.
- **Tests:**
  - **Unit (GDScript):** synthesised 200 KB payload; reassembler handles out-of-order.
  - **E2E:** initial sync of a 500 KB seeded doc.

### 4.4 — Awareness wire (tags 0/1 on `awareness`)
- DESIGN §Awareness protocol.
- **Tests:**
  - **E2E:** disconnect → peer's awareness entry pruned within 30 s ± 1 s.

### 4.5 — Control channel, compaction protocol
- **Goal:** compaction runs over its own channel for a clean mental model. Simple, loud.
- **Scope:**
  - Tags on `control` channel:
    - 0 `CompactionIntent(initiator_id)`
    - 1 `CompactionAck(peer_id)`
    - 2 `CompactionSwap(snapshot_bytes, new_doc_id)`
    - 3 `CompactionAbort(reason)`
  - Flow:
    1. Initiator calls `YSyncTransport.request_compaction()`.
    2. Sends `CompactionIntent`; local app sees `compaction_requested(initiator_id)` signal.
    3. Peers ACK once their app signals readiness (`YSyncTransport.ack_compaction()`).
    4. Initiator collects ACKs with a timeout (default 30 s); on timeout, `CompactionAbort` broadcast.
    5. On all-ACK: `CompactionSwap` with snapshot + new doc identity.
    6. Every peer atomically: close current doc, instantiate fresh doc, apply snapshot, re-emit `compaction_completed(new_doc)`.
  - **Loud:** every phase emits a signal; UI can show a "Compaction in progress — do not edit" banner.
  - **Invalidation:** every cached root accessor (`Gd<YText>` etc.) becomes invalid after swap — apps must re-fetch. Document in bold. Consider a `YDoc.is_valid()` probe that returns false post-swap, so wrappers can guard.
- **Tests:**
  - **E2E (3-peer):** session completes compaction; all three converge on identical new doc; in-flight edits at barrier are rejected with loud warning.
  - **E2E:** timeout path — one peer unreachable; initiator aborts within 30 s.
- **Depends on:** 4.1, 5.1 (snapshot API).

---

## Milestone 5 — Presence + history primitives

### 5.1 — Snapshot API
- `YDoc.encode_state_as_update()` + `YDoc.apply_update()` already exist; this phase is about making sure they're tested as first-class primitives and documented for caller use (disk checkpointing, new-peer initial-sync shortcut).
- **Tests:**
  - **Unit:** snapshot → fresh doc → `apply_update` → equal content.
  - **GUT:** seed doc from snapshot in `_ready`.

### 5.2 — `YRemoteCursors` overlay
- DESIGN §YRemoteCursors; overlay nodes for text controls.
- Per DESIGN: carets are indexed-property on `TextEdit`, single shared colour → overlay is the only option.
- **Tests:**
  - **E2E:** A moves caret → B's overlay updates within 150 ms.
  - **A.4 applies:** overlay has accessibility names.

### 5.3 — Structural presence overlays
- **Goal:** the presence story for non-text structural controls.
- **Scope:**
  - `YStructuralPresence` Node — generic overlay that listens to `YAwareness` and renders per-peer badges on:
    - Tab headers (who is on which tab).
    - Tree rows (who has which row selected/expanded).
    - Item-list rows.
    - Foldable container headers.
  - Badge = small dot + peer initial + peer colour; click → popup with full peer info.
  - Awareness state shape (under a stable `structural_focus` key):
    ```
    { "structural_focus": { "node_path": "...", "detail": {...} } }
    ```
- **Tests:**
  - **E2E:** two peers switch tabs; badges move; accessibility tree announces peer state changes.
- **Depends on:** 1.5, 3.x wrappers, A.4.

---

## Milestone 6 — Examples + relay

### 6.1 — Relay relocation
- Move `experiments/relay_server/` → `relay/`. Dockerfile + compose unchanged; docs call out TLS / real TURN creds / `external-ip` as production pre-reqs.

### 6.2 — Examples
- `01_text_sync` — single `YTextEdit` over relay. Two-subprocess runner in `godot/tests/e2e/`.
- `02_gui_sync` — a form of value binders + structural wrappers (tab container, split, foldable).
- `03_presence` — text + structural presence overlays.
- `04_compaction` — manual "compact now" button; loud banner.
- **Tests (per example):** E2E scripted flow, screenshot diff on golden paths.

---

## Milestone 7 — Multi-platform build

Priority per your direction. Each sub-phase is gated on the one before it going green in CI.

### 7.1 — Linux x86_64 (dev default)
- Already covered by 0.1.

### 7.2 — Windows x86_64
- MSVC toolchain in CI. Matrix add. `xtask build windows` cross-compiles from Linux via `cargo-xwin` **OQ** — or native Windows runner? *Lean: `cargo-xwin` on Linux runner; faster CI.*
- **Tests:** `01_text_sync` E2E on a Windows runner.

### 7.3 — Web (WASM: threaded + nothreads)
- Highest-risk platform; exp3 validated the path.
- Pin `emsdk`, Rust nightly, `rust-src`. `xtask build web` builds both variants.
- Threaded variant targets both mobile and desktop browsers per your priority.
- **Tests:**
  - **Playwright two-tab harness** against a locally-served Godot web export. Both variants run in CI.
  - Mobile browser run: headful BrowserStack or manual, **OQ**.

### 7.4 — macOS universal (x86_64 + arm64)
- `lipo` both arches. **No notarisation** on our side — downstream apps handle signing.
- Document for consumers: "this binary is unsigned; sign it as part of your app's notarisation."

### 7.5 — Android (arm64 + x86_64)
- `cargo-ndk`, pinned NDK. `xtask build android`.
- **Tests:** E2E on an Android emulator in CI **OQ** (slow; maybe manual).

### 7.6 — iOS arm64
- Static lib; Xcode wiring doc for consumers.
- **Tests:** manual; no CI emulation.

### 7.7 — Release pipeline
- Tag push → matrix build → `xtask package` → GitHub Release with binaries for every target that exists at tag time.

---

## Milestone 8 — Testing breadth & infra

Runs in parallel with feature milestones; this phase is about making the *infra itself* robust.

### 8.1 — Test-tier conventions
- **Rust unit:** `cargo test -p gyrs` — pure Rust, no Godot. Runs on every push.
- **GUT integration:** headless Godot, `godot/tests/{unit,integration}/`. Runs on every push.
- **E2E (GDScript subprocess harness):** `godot/tests/e2e/` — one orchestrator scene spawns N child Godot processes, scripts interactions through the real relay. Default path per your direction.
- **E2E (cargo infra fallback):** only when the GDScript harness gets messy or needs out-of-band fixtures — e.g. network fault injection, programmatic WebRTC snooping. Lives in `crates/gyrs-e2e/`.
- Documented in `docs/testing.md`.

### 8.2 — Observability in tests
- Every wrapper exposes a `debug_state()` dict (contents: local flags, buffered deltas, last observed delta id).
- E2E runner asserts on this dict, not on rendered pixels where avoidable.

### 8.3 — Accessibility CI
- Covered in A.5. Called out here so the tier list is complete.

### 8.4 — Perf regression suite
- Re-run exp5's three sub-experiments as CI jobs (nightly, not per-push). Fail PRs that regress >30 % on any metric.

---

## Post-1.0 (explicitly deferred per your direction)

- **IME active-compose matrix** (DESIGN OQ #3): per-platform manual verification; compose-time R1–R4 halves.
- **E2E encryption threat-model doc** (DESIGN OQ #1): what DTLS + TURN ciphertext gives you and doesn't.
- **Bounded-history mode**: ruled out on correctness grounds; re-evaluate only if there's a user report.

---

## Critical-path summary

```
0.1 ─→ 1.1 ─→ 1.2 ─→ 1.3 (reentrancy spike) ─┬─→ 1.4 (undo)
                                              ├─→ 1.5 (awareness)

2.1 (poller) ─→ 2.2 (binder) ─→ 2.3 (YTextEdit + fdelete spike) ─→ 2.4 (YLineEdit)

3.1 … 3.6 (structural wrappers) ─→ 3.7 (integration suite)

A.1 (audit) ─→ A.2 (preservation) ─→ A.3 (live regions) ─→ A.4 (presence) ─→ A.5 (CI)
     [runs in parallel with 2.x, 3.x, 5.x; gates each wrapper's "done"]

4.1 (peers + 3 channels) ─→ 4.2 (sync) ─→ 4.3 (chunking) ─→ 4.4 (awareness wire) ─→ 4.5 (compaction)

5.1 (snapshot) ─→ 5.2 (remote cursors) ─→ 5.3 (structural presence)

6.1 (relay) ─→ 6.2 (examples) ─→ 6.3 (compaction example)

7.1 (linux/dev) ─→ 7.2 (win) ─→ 7.3 (web) ─→ 7.4 (mac) ─→ 7.5 (android) ─→ 7.6 (ios) ─→ 7.7 (release)
```

Target for 1.0: Milestones 0–6 complete; A fully green; 7.1–7.3 in CI (Windows + WASM must ship); 7.4–7.6 best-effort; 8.x infra in place.

---

## Open questions summary

1. **Crate location:** `crates/gyrs/` (recommended) vs. root.
2. **Reentrancy deferred-call mechanism** — decided by the 1.3 spike; write-up in `docs/decisions/001-reentrancy.md`.
3. **Forward-delete path** — decided by the 2.3 spike; write-up in `docs/decisions/002-forward-delete.md`.
4. **`FoldableContainer` class name / availability** in Godot 4.6 (3.3).
5. **`Tree` scope** — structural-only (3.4) vs. structural + data; assuming structural-only with data-source pattern documented.
6. **`ButtonGroup` binder emits name vs. index** (2.2).
7. **AccessKit live-region API name** in Godot 4.6 — confirm `accessibility_live` exists (A.3).
8. **Minimum Godot version** for AccessKit needs we care about (A.1).
9. **`cargo-xwin` vs. native Windows runner** for 7.2.
10. **Mobile browser / Android emulator** coverage in CI (7.3, 7.5) — manual vs. BrowserStack vs. emulated.
11. **`YDoc.is_valid()` post-compaction** — expose as probe for wrappers, or rely on signal-driven invalidation? (4.5)
12. **Chunk-reassembly eviction** — confirm `peer_left` drop + 30 s absolute timer. (4.3)
13. **Structural presence awareness key name** — `structural_focus` (5.3); bikeshed-ready.
14. **Debounce default for split-container drag** — 20 Hz proposed (3.2).
15. **`YRemoteCursors` wrapping modes** — polish pass needed if `TextEdit.wrap_mode` variations regress (5.2).

DESIGN's own open questions: OQ #1 (E2E crypto docs) and #3 (IME matrix) stay deferred; OQ #2 (forward-delete) is now pulled into the 2.3 spike.
