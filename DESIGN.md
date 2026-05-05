# Godot CRDT Addon — Design

A Godot addon providing drop-in collaborative editing primitives over WebRTC: shared text, shared GUI state (maps and arrays), and presence (remote cursors, user metadata). Targets desktop, mobile, and web from a single codebase. This design is informed by the prototyping in `yrs-crdt-demo/` and `experiments/`.

---

## Goal

Install the addon, wrap existing `TextEdit` / `LineEdit` / `CheckBox` / `Slider` etc. with the provided classes, attach a `YSyncTransport` to a `WebRTCPeerConnection`, and collaboration works — no Rust knowledge required.

Target versions: Godot 4.6+, yrs 0.25+, gdext (master, pinned by revision).

## Non-goals

| Not doing | Why / what replaces it |
|---|---|
| `MultiplayerPeer` integration | `WebRTCMultiplayerPeer` is for RPCs and node replication; CRDT sync needs opaque binary with custom framing. The two coexist in one app, they don't compose. |
| Byte-for-byte Yjs wire compatibility | Goal is reusing proven protocol *design*, not cross-SDK interop. If formats end up compatible, bonus. |
| Persistence / storage | Out of scope. `YDoc.encode_state_as_update()` gives callers a snapshot; storing it is their problem. |
| Signaling server | External. A reference Rust+coturn image is bundled for dev; production is the user's choice. |
| First-class `Color`/`Vector2` as map values | Caller serializes (`color.to_html()`, `[x, y]`). Binders (see below) hide this for common controls. |

## Target matrix

| Platform | Arch | Notes |
|---|---|---|
| Linux | x86_64, arm64 | stable toolchain |
| macOS | x86_64, arm64 | universal binary |
| Windows | x86_64 | stable |
| Android | arm64, x86_64 | NDK via cargo-ndk |
| iOS | arm64 | static lib |
| Web | wasm32-unknown-emscripten | threaded + nothreads variants; nightly + `rust-src` |

---

## Validated constraints

These have passing experiments behind them and shape every design choice. Treat as hard constraints.

### UTF-16 offsets at the FFI boundary

yrs 0.25 offers `OffsetKind::Bytes` (UTF-8) and `OffsetKind::Utf16`. Godot strings are indexed by codepoint. The extension uses `Utf16` internally and converts at every boundary:

```rust
fn codepoint_to_utf16(s: &str, cp: usize) -> u32;
fn utf16_to_codepoint(s: &str, u: u32) -> usize;
fn utf16_len_to_codepoint_len(s: &str, u16_start: u32, u16_len: u32) -> usize;
```

BMP-only text (ASCII, CJK, Latin, combining chars) is identity. Only supplementary-plane characters (emoji, flag sequences) cost a linear walk. Negligible at typical document sizes. Source: `experiments/exp1_utf32_offsets/`.

### Observer callbacks must not mutate

`transact_mut()` inside a yrs observer callback **silently deadlocks** — no panic, no error, process hangs. `try_transact_mut()` returns `Err`. Even a read-only `try_transact()` fails. Source: `experiments/exp4_reentrancy/`.

Three-layer defense:

1. **Never call `transact_mut()` internally.** Every path uses `try_transact_mut()`; errors surface to GDScript via `godot_error!`.
2. **Deferred signal emission.** Observer callbacks push deltas into an `Arc<Mutex<Vec<_>>>` buffer and return. Signals fire from `poll_*()` methods the app calls in `_process()`, after the triggering transaction has committed.
3. **Reentrancy guard on public methods.** If a GDScript handler calls `y_text.insert()` from inside a `changed` signal, an `AtomicBool` per instance detects it, queues the mutation, and applies it via `call_deferred` the next frame. Depth-capped at 1. The same guard catches `YUndoManager.undo()` calls from observer handlers.

The prototype implements (1) and (2); (3) is the remaining piece and is mandatory before 1.0.

### WASM builds cleanly

yrs 0.25 compiles to `wasm32-unknown-emscripten` through gdext with zero source changes. `std::time::Instant` works on emscripten (POSIX `clock_gettime`), so awareness timeouts don't need `web-time`. Both `+atomics`/`-pthread` threaded and nothreads builds succeed. Release size ~900 KB for the full wrapper. Source: `experiments/exp3_wasm/`.

### Transport bypasses the high-level multiplayer API

`WebRTCMultiplayerPeer` expects RPC-style messages. CRDT sync wants opaque binary and explicit control over channel QoS. The addon owns its own `WebRTCPeerConnection` objects and creates **negotiated** DataChannels with fixed IDs. It never touches `multiplayer.multiplayer_peer`, which means an app can run both side by side (RPCs for gameplay, CRDT for shared document state). Source: prototype `crdt_peer_manager.gd`.

### Per-frame polling is cheap up to ~1000 instances

At N=100 bound controls, total poll cost is ~80 µs/frame whether polling per-instance (each Y\* wrapped in its own `_process`) or via a single `YPoller` autoload walking a `WeakRef` registry; at N=1000 it is still <500 µs. The amortized cost settles around 0.46 µs per `poll_*()` call, dominated by the FFI round-trip — Godot's `_process` dispatch overhead is not a bottleneck. The addon ships the autoload by default because it scales marginally better and the zero-config path matters more than the ~5 µs difference. Source: `experiments/exp5_perf/` §5A.

### Burst observer emission is O(transactions), not O(ops)

One `apply_update()` carrying a 19 KB multi-root diff (5 000-char text + 500 map keys + 500 array items, ~10 000 yrs ops total) collapses to **exactly one `changed` signal per root type** — three signals, each carrying the full delta array. End-to-end time from `apply_update()` call to last handler return is 4.4 ms including a real UI repaint; the single burst frame measures 4.7 ms. No batching, no chunked emission, no per-frame budget needed. The design's current "one signal per transaction, full delta array" strategy is load-bearing and stays. Source: `experiments/exp5_perf/` §5B.

### Awareness full-state broadcast does not scale; dirty tracking is the default

At N=10, 10 Hz, full-state broadcast costs ~21 KB/s combined per peer — >4× the 5 KB/s budget. Dirty tracking (encode only the fields that changed since last send) drops this to ~2.35 KB/s, a 9× reduction, and scales linearly. The default broadcast strategy is therefore dirty-tracked; full-state is available as an opt-in for small meshes or callers who want simpler code. The mesh cost is (N-1) × size × Hz, not (N-1) × size × 1 — the original estimate in this doc (~4.5 KB/s at N=10) was off by a factor of ~5, the factor is now corrected. Source: `experiments/exp5_perf/` §5C.

### TextEdit override mechanics (non-IME)

Mechanical IME coverage (`experiments/exp6_ime/`) locked down idle invariants and grapheme-cluster navigation, plus several override semantics that shape the YTextEdit implementation. These hold regardless of IME:

- **`_handle_unicode_input` is dispatched once per input event, with `caret_index = -1` when multiple carets are active** — not N separate calls. The engine's default handler fans out internally via `insert_text_at_caret(text, -1)`. A YTextEdit override must either delegate to that same call or iterate `get_caret_count()` manually; assuming per-caret dispatch silently drops edits on multi-caret scenes.
- **Native engine virtuals cannot be reached via `super.*` from GDScript** (`_handle_unicode_input` resolves to a C++ symbol invisible to the GDScript parser). Overrides must replicate the default behavior in full: `delete_selection(caret_index)` if applicable, then `insert_text_at_caret(chr, caret_index)`.
- **`insert_text_at_caret` does not emit `text_changed`** in Godot 4.6. An override that uses it to stage the character silently breaks any downstream `text_changed` listener. Options: emit `text_changed` manually after the insert, or don't use `text_changed` as a commit path at all — the addon prefers the latter and relies on its own `YText.changed` signal.
- **`text_changed` is deferred one frame** after engine-dispatched unicode input. Tests asserting on it must yield a frame; logic requiring same-frame reaction to keystrokes must poll or hook `_gui_input`, not `text_changed`.
- **Grapheme-cluster navigation via `get_next/previous_composite_character_column` is fully verified** across ASCII, regional-indicator flags, skin-tone sequences, ZWJ families, and combining accents. `backspace_deletes_composite_character_enabled = true` is the only correct default — the disabled path produces half-clusters (orphan regional indicators, detached ZWJs, stranded combining marks) that no peer should have to reconcile.
- **`LineEdit.max_length` overflow fires `text_change_rejected`** as a real observable event, not a silent drop — the addon can rely on that signal for bounded-input binders rather than polling.

IME-active behavior — preedit side-buffering, compose transitions on `has_ime_text()`, per-character dispatch on IME commit — still requires the per-platform manual matrix; open question 3 tracks it.

---

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│ App code (GDScript)                                         │
│                                                             │
│  YTextEdit / YLineEdit / YBinder-backed controls            │
│                          ↕                                  │
│  YDoc / YText / YMap / YArray / YUndoManager / YAwareness   │  ← GDExtension boundary
│                          ↕                                  │
│                   PackedByteArray                           │
│                          ↕                                  │
│  YSyncTransport (sync + awareness protocol)                 │
│                          ↕                                  │
│  CRDTPeerManager (WebRTCPeerConnection + DataChannels)      │
│                          ↕                                  │
│  ws_webrtc_client (WebSocket signaling)                     │
│                          ↕                                  │
│                   External signaling server                 │
└─────────────────────────────────────────────────────────────┘
```

Three strict layers. Each has one responsibility and one contract with the next:

- **Rust GDExtension** — pure CRDT + FFI. No Godot networking imports, no knowledge of DataChannels.
- **Control wrappers** — subclass stock Godot controls; know about the extension API, nothing else.
- **Transport** — pure Godot (WebRTC + polling). Moves `PackedByteArray` blobs between `YDoc`/`YAwareness` and peer channels.

---

## Rust GDExtension: `yrs_godot`

### Crate layout

```
yrs_godot/
  Cargo.toml                    # cdylib, native + WASM targets
  src/
    lib.rs                      # ExtensionLibrary entry
    core/                       # pure Rust, unit-testable
      offsets.rs                # UTF-16 <-> codepoint
      awareness.rs              # Awareness protocol (reimplemented)
      sync_protocol.rs          # Message type constants
      reentrancy.rs             # AtomicBool guard + deferred queue
    godot_api/                  # #[derive(GodotClass)]
      doc.rs                    # YDoc
      text.rs                   # YText
      map.rs                    # YMap
      array.rs                  # YArray
      undo_manager.rs           # YUndoManager
      awareness.rs              # YAwareness (Godot wrapper)
      conversions.rs            # Variant <-> yrs::Any / yrs::Out
```

### YDoc

```gdscript
class YDoc extends RefCounted

static func create(client_id: int = 0, gc: bool = true) -> YDoc    # 0 = random

# Root-type accessors; idempotent, instance-cached
func get_text(name: String) -> YText
func get_map(name: String) -> YMap
func get_array(name: String) -> YArray

# Sync primitives — opaque binary
func encode_state_vector() -> PackedByteArray
func encode_diff(remote_sv: PackedByteArray) -> PackedByteArray
func encode_state_as_update() -> PackedByteArray   # full snapshot
func apply_update(update: PackedByteArray) -> bool

# Observability
signal update_produced(update: PackedByteArray)
func poll_updates()                                 # drain + emit

func get_client_id() -> int
```

Internal: `observe_update_v1` pushes to a buffer; `poll_updates()` drains and emits. Root accessors cache `Gd<T>` wrappers so repeated `get_text("content")` calls return the same instance — signal connections stay valid.

**Origins.** Every mutation routed through a child `YText`/`YMap`/`YArray` opens its transaction with `transact_mut_with(client_id)`. This tags local ops with the doc's own client ID, making undo-scope filtering correct without caller involvement. `apply_update()` does not set an origin, so remote ops never get tagged as local.

**GC.** `gc: true` (default) enables automatic tombstone compaction — deleted content's *payload* is dropped while a minimal ID reference is retained for concurrency resolution. Disable only if you need full history preservation (e.g., audit-log overlay). See "History management and compaction" below.

### YText

```gdscript
class YText extends RefCounted

func insert(index: int, text: String)               # codepoint index
func delete(index: int, length: int)                 # codepoint length
func push(text: String)
func format(index: int, length: int, attrs: Dictionary)   # attributed text

func get_string() -> String
func length() -> int                                 # codepoints

signal changed(deltas: Array[Dictionary])
func poll_changes()
```

Delta entries:
```
{ "type": "insert", "value": "hello" }
{ "type": "delete", "value": 4 }          # codepoint count
{ "type": "retain", "value": 10 }
```

Deltas are translated from UTF-16 to codepoint lengths before emission.

### YMap

```gdscript
class YMap extends RefCounted

func set(key: String, value: Variant)
func get(key: String, default: Variant = null) -> Variant
func has(key: String) -> bool
func delete(key: String)
func keys() -> PackedStringArray
func to_dict() -> Dictionary
func size() -> int

signal changed(changes: Dictionary)
func poll_changes()
```

Supported value types: `nil`, `bool`, `int`, `float`, `String`, `PackedByteArray`, `Array` (recursive), `Dictionary` with String keys (recursive). Anything else emits a Godot error. Dicts with non-String keys error out.

`changes` shape:
```
{
  "key_name": { "action": "insert"|"update"|"delete",
                "old_value": Variant, "new_value": Variant }
}
```

### YArray

```gdscript
class YArray extends RefCounted

func insert(index: int, value: Variant)
func insert_range(index: int, values: Array)
func push(value: Variant)
func delete(index: int, length: int = 1)
func get(index: int) -> Variant
func size() -> int
func to_array() -> Array

signal changed(deltas: Array[Dictionary])
func poll_changes()
```

Deltas match YText shape (insert/delete/retain), with `value` as a Variant array for inserts.

### YUndoManager

```gdscript
class YUndoManager extends RefCounted

static func create(scope: Array, options: Dictionary = {}) -> YUndoManager

# Options:
#   capture_timeout_ms: int = 500    # time-based action grouping
#   track_all_origins: bool = false  # default: only local origin

# Core
func undo() -> bool
func redo() -> bool
func can_undo() -> bool
func can_redo() -> bool

# Grouping
func reset()                          # close current frame; next op starts a new one

# Origins (advanced)
func include_origin(origin: int)
func exclude_origin(origin: int)

func clear()

signal stack_changed()
```

Wraps yrs's `UndoManager`. Four things to know:

**Scope.** `scope` is an `Array` of `YText` / `YMap` / `YArray` instances — at least one is required. The manager only tracks ops on those references; anything outside the scope is invisible to undo/redo. There is no "undo everything across the doc" shortcut; callers opt each collection in explicitly.

**Origins.** Every local mutation in the extension tags its transaction with the doc's `client_id` via `transact_mut_with`. `YUndoManager.create(...)` automatically calls `include_origin(client_id)` so only local ops land on the stack — remote updates applied through `apply_update()` are skipped. This matches the Yjs convention. Set `track_all_origins: true` in options for "git-style" undo where every peer's ops go on the stack (rarely what you want).

**Grouping.** yrs merges rapid-fire ops (each keystroke is its own transaction) into a single undo frame using a time window, default 500 ms, tunable via `capture_timeout_ms`. `reset()` forces a frame boundary — call it after a "Save", a mode switch, or any natural break so the next undo doesn't swallow unrelated work.

**Transaction safety.** `undo()` and `redo()` open their own transactions internally, so the caller **must not** hold one open when calling them. In particular: never call `undo()` from an observer signal handler — the reentrancy guard (layer 3) catches this and defers via `call_deferred`, but the semantics get confusing if you rely on it. Route undo from UI actions, not from `changed` handlers.

**Remote sync interaction.** Undoing a local op produces a new update (a deletion on the CRDT graph) that broadcasts to all peers normally. The CRDT merge handles this correctly even if other peers have made concurrent edits on top of the undone op. Redo re-inserts.

### YAwareness

```gdscript
class YAwareness extends RefCounted

static func create(doc: YDoc) -> YAwareness

# Local
func set_local_state(state: Dictionary)             # full replacement
func set_local_field(key: String, value: Variant)   # partial update
func clear_local_state()
func get_local_state() -> Dictionary
func get_local_client_id() -> int

# Remote
func get_states() -> Dictionary                      # client_id -> Dictionary
func get_clients() -> PackedInt64Array

# Wire protocol
enum BroadcastStrategy { FULL_STATE, DIRTY_TRACKED }
func set_broadcast_strategy(strategy: BroadcastStrategy)     # default: DIRTY_TRACKED
func encode_update(clients: PackedInt64Array = []) -> PackedByteArray   # [] = all; honours strategy
func encode_full_update(clients: PackedInt64Array = []) -> PackedByteArray  # forces full-state regardless of strategy
func apply_update(update: PackedByteArray) -> bool

# Housekeeping
func tick()                                          # prune stale clients (call ~1Hz)

signal changed(added: PackedInt64Array,
               updated: PackedInt64Array,
               removed: PackedInt64Array)
```

**Protocol.** Reimplemented in `core/awareness.rs` — the `y-sync` crate is archived and pinned to yrs 0.17. Per-client: monotonic `clock: u64`, JSON `state` blob. LWW on `clock`. 30-second staleness timeout, pruned on `tick()`.

**Wire format.** `varint(n_clients)` + `n` × `(varint(client_id), varint(clock), varint(state_len), state_bytes)`. Compact; similar in spirit to y-protocols awareness update but not byte-compat by contract.

**Broadcast strategy.** Default `DIRTY_TRACKED`: `encode_update()` compares the current local state against the last-sent snapshot (held Rust-side, not exposed to GDScript) and encodes only fields that changed, wrapping them in the same wire format as a full update — the recipient's `apply_update` is agnostic. `FULL_STATE` encodes the entire local state every call. The Rust side owns the diff state so callers never have to round-trip through GDScript to produce a delta (experiment 5C's GDScript diff destroyed canonical local state; the production implementation preserves it by diffing before encode and snapshotting the post-encode state). `encode_full_update()` is the escape hatch for new-peer joins, where a partial update is meaningless to a peer that has no baseline — the `AwarenessQuery` path always answers with a full update regardless of strategy. Source: `experiments/exp5_perf/` §5C.

### Conversions

At the FFI boundary, `Variant` ↔ `yrs::Any`:

| Variant | yrs::Any |
|---|---|
| NIL | Null |
| BOOL | Bool |
| INT | BigInt (i64) |
| FLOAT | Number (f64) |
| STRING | String |
| PACKED_BYTE_ARRAY | Buffer |
| ARRAY | Array (recursive) |
| DICTIONARY (String keys) | Map (recursive) |

Everything else: error.

For `YMap.get()` / `YArray.get()`, if the value is a nested CRDT type (`yrs::Out::YText`/`YMap`/etc.) the extension returns a wrapper `YText`/`YMap`/`YArray` instance rather than erroring — enables nested shared types.

---

## GDScript layer

### The local/remote pattern

Every editable control wrapper uses this flag dance to avoid feedback loops:

```gdscript
var _applying_remote := false
var _local_edit := false

# Remote change arrives:
#   _applying_remote = true
#   update displayed state
#   _applying_remote = false

# Local input fires:
#   if _applying_remote: return
#   _local_edit = true
#   y_thing.mutate(...)
#   _local_edit = false

# Observer signal handler:
#   if _local_edit: return       # we already updated display
#   if _applying_remote: return
#   _applying_remote = true
#   apply delta to display
#   _applying_remote = false
```

The Rust reentrancy guard is the safety net; this flag dance is the primary flow.

### YTextEdit

```gdscript
class_name YTextEdit extends TextEdit

func bind(y_text: YText, undo_manager: YUndoManager = null)
```

Overrides these `TextEdit` virtuals (full list from Godot 4.6 bindings): `_handle_unicode_input`, `_backspace`, `_cut`, `_copy`, `_paste`, `_paste_primary_clipboard`. There is no `_delete` virtual — forward delete reaches TextEdit as a raw `KEY_DELETE` `InputEventKey` and must be intercepted via `_gui_input`. The `EditAction` enum (`ACTION_NONE`, `ACTION_TYPING`, `ACTION_BACKSPACE`, `ACTION_DELETE`) is Godot's internal action classification (used by the built-in undo stack, which we disable) and is not a signal that every action has a virtual override.

Each intercept:

1. Bail if `_applying_remote`.
2. Translate caret `(line, col)` → codepoint offset via `get_line()` walk, respecting grapheme clusters with `get_next_composite_character_column` / `get_previous_composite_character_column`.
3. Mutate the bound `YText`.
4. Refresh display (`text = y_text.get_string()` inside `_applying_remote = true`).
5. Restore caret at the new offset.

Built-in TextEdit undo is disabled — undo routes through `YUndoManager`. Clipboard cut/paste still uses `DisplayServer.clipboard_*`.

**Override shape (non-IME).** These rules come from the mechanical pass in `experiments/exp6_ime/` and hold regardless of whether an IME is active:

- **Handle `caret_index == -1` as "all carets".** One key event fires exactly one `_handle_unicode_input` call; the engine signals multi-caret via the sentinel, not by dispatching N times. The override delegates to `insert_text_at_caret(chr, caret_index)` which fans out internally when `caret_index == -1`.
- **Replicate the default behavior in full — there is no `super._handle_unicode_input()`.** Native virtuals are C++ symbols invisible to GDScript. The override does `if has_selection(caret_index): delete_selection(caret_index)` then `insert_text_at_caret(chr, caret_index)`, in addition to its CRDT bookkeeping.
- **Do not rely on `text_changed` as a commit signal.** `insert_text_at_caret` does not fire it (Godot 4.6); `text_changed` is also deferred one frame after engine-dispatched input. The addon syncs through its own `YText.changed` path and uses `text_changed` only as a best-effort second belt, never as the primary commit hook.
- **Set `backspace_deletes_composite_character_enabled = true` at `bind()` time** and use `get_next/previous_composite_character_column` for all caret arithmetic after edits. The disabled path produces half-flags / orphan ZWJs / stranded combining marks, which the CRDT has no business reconciling.

**IME handling (compose path).** TextEdit renders preedit in a side buffer (`get_line_with_ime()` returns the line *with* preedit; `get_line()` does not), so composition never touches our CRDT. Committed IME input is expected to dispatch through `_handle_unicode_input` per committed character, and the standard intercept path then works unchanged.

Two IME-specific rules the intercept code must follow:

- **Do not refresh `text` while `has_ime_text()` returns true.** If a remote update arrives mid-composition, `text = y_text.get_string()` would clobber the visible preedit. Gate the refresh: either skip until composition ends, or call `apply_ime()` to force commit before refreshing.
- **`apply_ime()` / `cancel_ime()` are safe to call when no composition is active** — verified as no-ops (no text mutation, no signal fire). The addon can call `apply_ime()` on focus-loss or before a forced refresh without worrying about phantom commits.

The idle halves of these rules (R2/R3/R4 from the experiment) and the grapheme-cluster navigation (R5) are mechanically verified. The active-compose halves — preedit side-buffering, `has_ime_text()` transitions, per-character dispatch on commit, `apply_ime()` / `cancel_ime()` mid-composition — require a real IME and belong to the manual platform matrix (open question 3).

### YLineEdit

Same pattern. `LineEdit` exposes no input virtuals, so interception is via `_gui_input` only. The IME surface is parallel to `TextEdit` (`apply_ime` / `cancel_ime` / `has_ime_text` / composite-character navigation / backspace-deletes-composite toggle), and the same rules above apply. `text_change_rejected` fires on `max_length` overflow (verified), so a YLineEdit bound to a length-capped `YText` can observe overflow reliably rather than polling.

### YBinder

Rather than a wrapper class per control type, a single binder helper:

```gdscript
class_name YBinder extends Node

func bind_checkbox(cb: CheckBox, y_map: YMap, key: String, default: bool = false)
func bind_slider(s: Range, y_map: YMap, key: String, default: float = 0.0)
func bind_line_edit(le: LineEdit, y_map: YMap, key: String, default: String = "")
func bind_color_picker(cp: ColorPickerButton, y_map: YMap, key: String, default: Color = Color.WHITE)
func bind_option_button(ob: OptionButton, y_map: YMap, key: String, default: int = 0)
func bind_spin_box(sb: SpinBox, y_map: YMap, key: String, default: float = 0.0)
```

Each binder hooks the control's outbound change signal and listens to `y_map.changed` inbound, applying the flag pattern. `Color` is serialized as `#RRGGBBAA`; `Vector2`/`Vector3` as fixed-length arrays.

### YPoller

```gdscript
class_name YPoller extends Node

# Registered as an autoload by the plugin; apps can also instantiate manually.
func register(instance: Object) -> void   # YDoc / YText / YMap / YArray / YAwareness
func unregister(instance: Object) -> void
```

A single Node that holds a `WeakRef` list of Y\* instances and calls their `poll_*()` methods once per frame in `_process()`. At N=100 instances, this is ~80 µs/frame (see "Per-frame polling is cheap" under Validated constraints); per-instance polling is within a few µs of that. The autoload is the default because the zero-config path matters more than the margin, and dead references drop out automatically via `WeakRef`. Apps that want per-instance polling can skip the autoload — `poll_updates()` / `poll_changes()` remain public.

Control wrappers (YTextEdit, YLineEdit, YBinder) register their bound Y\* handle with the autoload at `bind()` time and unregister on `_exit_tree`.

### YRemoteCursors

```gdscript
class_name YRemoteCursors extends Node

func attach_text_edit(text_edit: TextEdit, awareness: YAwareness, cursor_key: String = "cursor")
```

Draws remote cursors and selections over a `TextEdit` by spawning overlay nodes (`ColorRect` for the caret, translucent `ColorRect` rows for selection, `Label` for the peer name) positioned via `TextEdit.get_line_height()` + column-width math.

Overlay rendering is the only workable approach: `TextEdit` exposes carets as an *indexed property* of the control (`get_caret_count()`, `add_caret()`, `get_caret_line(index)`) rather than as addressable instances, and caret color is a single theme-wide field shared across all caret indices. Per-peer coloring is not achievable through the built-in multi-caret machinery.

Awareness state shape read by the overlay:

```
{
  "name": "alice",
  "color": "#ff8040",
  "cursor": { "line": 12, "column": 5 },
  "selection": { "from_line": 12, "from_col": 0, "to_line": 12, "to_col": 5 }
}
```

Local cursor broadcast is rate-limited to 10 Hz and triggered by `caret_changed` plus selection signals.

---

## Transport layer

### Channel topology

Per peer, two negotiated DataChannels with fixed IDs:

| ID | Label | Mode | Purpose |
|---|---|---|---|
| 1 | `doc_sync` | reliable, ordered, binary | CRDT state vectors + diffs + updates |
| 2 | `awareness` | unreliable, unordered, binary | Presence updates |

Negotiated (`negotiated: true`, matching IDs on both sides) — avoids the `ondatachannel` race inherent in non-negotiated channels.

### Doc sync protocol

Single-byte type tag. Tags 0–2 match the prototype; tag 3 is added for chunking.

| Tag | Name | Payload |
|---|---|---|
| 0 | SyncStep1 | state vector |
| 1 | SyncStep2 | diff from sender for receiver's SV |
| 2 | Update | post-sync incremental broadcast |
| 3 | UpdateChunk | fragmented update (see below) |

Handshake when a peer's `doc_sync` channel opens:

1. Send SyncStep1 (local state vector).
2. On receiving SyncStep1: reply SyncStep2; if peer not yet marked synced, also send our own SyncStep1.
3. On receiving SyncStep2: `apply_update`, mark peer synced, emit `peer_synced(id)`.
4. Thereafter: forward every `YDoc.update_produced` to all peers as Update (tag 2).

### Large-update chunking

Reliable DataChannel messages are capped around 64 KB (Chrome) to 256 KB (Firefox). Initial sync of a large doc can exceed this.

UpdateChunk framing (tag 3):

```
[3][u32 update_id][u16 chunk_index][u16 total_chunks][bytes...]
```

Threshold: fragment any SyncStep2 or Update payload >48 KB. The transport reassembles in-order per `update_id` and calls `apply_update` once all chunks arrive. Tags 1 and 2 are reserved for single-message updates; the transport transparently switches to tag 3 when payload exceeds the threshold.

### Awareness protocol

On the awareness (unreliable) channel, same one-byte tag:

| Tag | Name | Payload |
|---|---|---|
| 0 | AwarenessUpdate | `YAwareness.encode_update()` output |
| 1 | AwarenessQuery | empty — requests a full-state broadcast |

Local awareness broadcasts:
- On every `set_local_state` / `set_local_field`, rate-limited to 10Hz.
- On peer join: send AwarenessQuery → receive full AwarenessUpdate.
- `tick()` runs at 1Hz to prune clients >30s stale.

### YSyncTransport API

```gdscript
class_name YSyncTransport extends Node

signal peer_synced(peer_id: int)
signal peer_left(peer_id: int)
signal compaction_requested(initiator_id: int)

func setup(doc: YDoc, awareness: YAwareness, peer_manager: Node) -> void
func request_compaction() -> void                    # see History management
```

`peer_manager` must:
- Expose `_peers: Dictionary` keyed by `peer_id → { doc_channel, awareness_channel }`.
- Emit `data_channel_message(peer_id: int, channel_id: int, data: PackedByteArray)`.
- Emit `peer_left(peer_id: int)`.

The reference implementation `CRDTPeerManager` extends `ws_webrtc_client.gd` and handles channel creation; apps can swap it for their own WebRTC wiring as long as they meet the contract.

### Peer lifecycle

```
signaling: peer_connected(id)
  → WebRTCPeerConnection created, ice_servers configured
  → doc_sync + awareness channels created (negotiated, ids 1 & 2)
  → if local_id > remote_id: create_offer; else wait
doc_sync opens
  → transport.initiate_sync(id)
awareness opens
  → transport.broadcast_awareness_query()
peer disconnected
  → close WebRTCPeerConnection
  → transport clears synced_peers[id], emits peer_left
  → YUndoManager drops frames tagged with that origin
```

---

## History management and compaction

A long-lived collaborative document accumulates operation history. This is inherent to CRDTs — every insertion and deletion is a distinct operation with a unique ID, and some structural metadata about deleted items must be retained to resolve concurrent ops that reference those positions. Unbounded history means unbounded memory and ever-slower sync. The addon offers three knobs for bounding this cost; they compose.

### 1. Automatic GC (default on)

yrs removes the *content* of deleted items while keeping a minimal tombstone for their ID. Controlled by the `gc` flag on `YDoc.create(client_id, gc = true)`. Disable only if you need full history preservation.

- **GC on:** storage grows with total ops performed, but each deleted op compacts to a few tens of bytes regardless of deleted-content size. A doc with 100k edits and 99% deletion is roughly the same size as one with 1k edits and no deletion.
- **GC off:** storage grows with total bytes ever inserted. Don't do this unless you're building an audit-log overlay.

### 2. Snapshot export

`YDoc.encode_state_as_update()` returns a compact binary encoding of the current state. Typically much smaller than the cumulative update log because merged runs collapse and GC'd tombstones compact further. This is the right payload to:

- Save to disk as a checkpoint.
- Ship to a brand-new peer joining a session (bypass the state-vector handshake entirely).
- Log before a destructive action.

A snapshot is not compaction on its own: loading one into a fresh `YDoc` and then applying further updates still grows history. But snapshots cap the *on-disk / network-transfer* cost independent of in-memory op count.

### 3. Compaction cycle (snapshot rotation)

The hard reset. When a session wants to drop history entirely:

1. All peers converge (application-level barrier — no in-flight updates).
2. Host calls `YDoc.encode_state_as_update()` to get a snapshot.
3. All peers discard their current doc, create a fresh `YDoc` with a **new** document identity, and apply the snapshot.
4. Sync resumes on the new doc.

This breaks concurrent edits in flight during the cycle, so it requires application-level coordination. `YSyncTransport.request_compaction()` broadcasts an intent over `doc_sync`, waits for ACKs, and swaps the doc atomically once all peers are ready. Apps schedule this at natural quiescent moments — save-and-exit, session boundaries, explicit "Compact history" user action.

### 4. Client-ID stability

Each peer's state vector grows with the number of distinct client IDs that have ever edited the doc, not just active peers. Transient-user apps (open doc, make three edits, close) can see this grow without bound.

Mitigation: reuse `client_id` across sessions when you can — pass a stable per-user ID to `YDoc.create(client_id)` rather than letting it default to random. One ID per human user, not one per session.

### What's *not* supported

- **Bounded-history mode.** There is no "keep only the last N operations" knob. CRDT correctness requires the full op graph to merge concurrent edits; capping arbitrarily breaks the invariants. The snapshot-rotation pattern above is the supported way to cap history.
- **Partial history truncation.** You can't "roll up" edits older than timestamp T while preserving concurrent edits in flight — same invariant.
- **Branching / forking.** Out of scope. Use application-level document copies if you need this.

### Practical guidance

- **Short-lived sessions** (a meeting, an editing session): don't compact. GC handles it.
- **Medium-lived docs** (hours, occasional reconnect): rely on GC + snapshots for new-peer sync.
- **Long-lived docs** (days+, many participants): schedule snapshot rotations at quiet moments, and keep `client_id` stable per user.

---

## Signaling

External. The addon is agnostic as long as the app can surface a `WebRTCPeerConnection` per peer. The bundled reference uses the same protocol as stock Godot `webrtc_signaling`:

```
{ "type": <int>, "id": <int>, "data": <string> }
```

Commands: JOIN, ID, PEER_CONNECT, PEER_DISCONNECT, OFFER, ANSWER, CANDIDATE, SEAL.

The bundled Rust implementation at `relay/` runs WebSocket signaling + coturn (STUN/TURN) in one Docker image via supervisord. Dev-only: production deployments must add TLS (WSS + TURNS), real credentials, `external-ip` for coturn behind NAT, and a wider TURN relay port range.

---

## Build & distribution

### `.gdextension`

```ini
[configuration]
entry_symbol = "gdext_rust_init"
compatibility_minimum = 4.6

[libraries]
linux.x86_64           = "res://addons/yrs_godot/bin/linux/libyrs_godot.so"
linux.arm64            = "res://addons/yrs_godot/bin/linux/libyrs_godot_arm64.so"
macos                  = "res://addons/yrs_godot/bin/macos/libyrs_godot.dylib"
windows.x86_64         = "res://addons/yrs_godot/bin/windows/yrs_godot.dll"
android.arm64          = "res://addons/yrs_godot/bin/android/libyrs_godot_arm64.so"
android.x86_64         = "res://addons/yrs_godot/bin/android/libyrs_godot_x86_64.so"
ios.arm64              = "res://addons/yrs_godot/bin/ios/libyrs_godot.a"
web.wasm32             = "res://addons/yrs_godot/bin/web/yrs_godot.threads.wasm"
web.wasm32.nothreads   = "res://addons/yrs_godot/bin/web/yrs_godot.wasm"
```

### Addon layout

```
addons/yrs_godot/
  plugin.cfg
  yrs_godot.gdextension
  bin/                               # per-platform binaries (gitignored, CI-produced)
  scripts/
    transport/
      y_sync_transport.gd
      crdt_peer_manager.gd
      ws_webrtc_client.gd
    controls/
      y_text_edit.gd
      y_line_edit.gd
      y_binder.gd
      y_remote_cursors.gd
  scenes/
    crdt_peer_manager.tscn
    y_remote_cursors.tscn
  examples/
    01_text_sync/
    02_ui_sync/
    03_presence/
```

### Build pipeline

```
cargo xtask build linux
cargo xtask build macos
cargo xtask build windows
cargo xtask build android            # via cargo-ndk
cargo xtask build ios
cargo xtask build web                # builds both threaded + nothreads
cargo xtask build all
cargo xtask package                  # strips, collects into addons/yrs_godot/bin/
```

Native targets use stable Rust. WASM requires `emsdk` + nightly with `rust-src`. CI matrix produces release binaries for every target on tag push and attaches them to the GitHub release.

---

## Testing

### Rust unit (fast, no Godot)

- `core/offsets.rs` — codepoint/UTF-16 round-tripping: ASCII, CJK, emoji, combining chars, mixed, empty.
- `core/awareness.rs` — clock LWW, staleness pruning, wire encode/decode round-trip, multi-client merge.
- `godot_api/conversions.rs` — Variant/Any round-tripping for every supported type, including nested Array/Map.
- `core/reentrancy.rs` — guard flag, deferred queue draining, depth cap.

### Integration (GUT, headless Godot)

- YDoc: creation, client_id, state vector non-empty, encode_diff + apply_update convergence, snapshot round-trip via `encode_state_as_update`.
- YText/YMap/YArray: CRUD, observer signals, two-doc sync (unidirectional + bidirectional convergence with conflicting ops).
- YUndoManager: undo/redo respects origin boundaries (local undo doesn't touch remote ops); `reset()` boundary correctness; time-grouping.
- YAwareness: set/get, encode/apply round-trip, `tick()` pruning at faked-time boundary.
- Chunking: synthesize >100 KB update, send through mock transport, verify reassembly + `apply_update` equality.
- Compaction cycle: three-peer session, trigger `request_compaction`, verify new doc identity and convergent state.

### End-to-end

Headless two-client mesh against the bundled relay:

- Type in A → appears in B within 100ms.
- Toggle checkbox in A → flips in B.
- Disconnect A → B sees cursor removed within 30s.
- Load 10k-char doc into A, have B join → B's sync completes, no dropped chunks.

### Browser

Playwright harness driving two tabs with Godot web exports, same scenarios. Both WASM variants (threaded + nothreads) exercised.

---

## Open questions

Ordered by priority for 1.0.

1. **End-to-end encryption.** WebRTC provides DTLS peer-to-peer, but a TURN relay sees ciphertext only, not plaintext — meaning "end-to-end" is already true against an untrusted relay at the transport layer. Out of scope to add application-layer crypto on top, but document the boundary clearly so app authors understand the threat model.
2. **Forward-delete interception path.** `TextEdit` has no `_delete` virtual, so forward-delete reaches us as a raw `InputEventKey` (`KEY_DELETE`) through `_gui_input`. Two plausible approaches: (a) intercept in `_gui_input`, consume the event, emit the equivalent `YText.delete` ourselves; (b) let TextEdit apply its own delete, then reconcile by diffing `text` against the YText state in `_process` (the `text_changed` path can't be relied on — see "TextEdit override mechanics" above). Option (a) is cleaner but requires handling all the edge cases the built-in delete handles (selections, end-of-line merges, composite clusters). Deferred until after higher-priority work.
3. **IME composition — per-platform manual matrix.** `experiments/exp6_ime/` mechanically verified the idle invariants of R2/R3/R4, grapheme-cluster navigation (R5), multi-caret dispatch shape, and the no-`super.*` / silent-`text_changed` footguns (all now in Validated constraints above). The **compose-time halves** of R1–R4 and scenarios S2–S6, S8 (IME half), S9–S12 require a real IME and are not mechanizable: does `_handle_unicode_input` really fire once per committed character on a Pinyin / Japanese / Korean commit? Does `get_line_with_ime` diverge from `get_line` during compose on every platform (Android InputConnection is the usual suspect)? Does `has_ime_text()` flip reliably at compose start and end? Does `apply_ime()` mid-compose synthesize per-char dispatches or commit silently? Platforms: Linux fcitx5/IBus, Windows IME, macOS native, Android Gboard at P0; iOS, Web at P1/P2. Any "broken" result gets a `_gui_input` workaround; any "partial" gets an amendment to the YTextEdit section above.

### Resolved in this revision

- ~~Per-frame poll overhead~~ — Both per-instance and autoload variants pass the <1 ms/frame target at N=100 (~80 µs) and N=1000 (<500 µs). The addon ships the `YPoller` autoload by default; per-instance stays available. Source: `experiments/exp5_perf/` §5A.
- ~~Deferred-emission burst cost~~ — A 19 KB / 10 k-op `apply_update` collapses to three signals (one per root) and completes end-to-end including UI repaint in 4.4 ms; worst frame 4.7 ms. The "one signal per transaction, full delta array" strategy stays; no batching machinery needed. Source: `experiments/exp5_perf/` §5B.
- ~~Awareness bandwidth at scale~~ — Full-state at 10 Hz, N=10 costs ~21 KB/s/peer combined — the original estimate was off by the Hz factor. Dirty tracking drops this to ~2.35 KB/s (9× reduction). Default is dirty-tracked; full-state is an opt-in via `YAwareness.set_broadcast_strategy(FULL_STATE)`. The Rust side owns the diff state so canonical local state is preserved across encodes. Source: `experiments/exp5_perf/` §5C.
- ~~Per-caret TextEdit coloring~~ — Carets are an indexed property of the `TextEdit` with a single shared color, not distinct instances. Remote-cursor rendering uses overlay nodes exclusively.
- ~~Origin tagging API~~ — Every local mutation opens its transaction via `transact_mut_with(client_id)`. `YUndoManager.create(...)` auto-calls `include_origin(client_id)`, so remote ops are excluded from the local undo stack by default. Time-based grouping (500 ms default) is configurable via `capture_timeout_ms`; explicit boundaries via `reset()`.
- ~~Snapshot API shape~~ — `YDoc.encode_state_as_update()` is the snapshot primitive. Reload is `YDoc.create()` followed by `apply_update(snapshot)`. The compaction cycle pattern is documented in "History management and compaction" above.
- **IME integration approach confirmed (compose-time coverage pending).** `TextEdit` and `LineEdit` expose `apply_ime()` / `cancel_ime()` / `has_ime_text()` / `get_line_with_ime()` plus composite-character navigation; the addon integrates with the platform IME rather than reimplementing composition tracking. Idle invariants and override mechanics are locked in by mechanical tests. Per-platform compose-time verification is tracked as open question 3 above.

---

## What the prototype already validates

For reference when reviewing this design against the existing code in `yrs-crdt-demo/`:

| Design element | Prototype status |
|---|---|
| YDoc with observe_update_v1 + buffered emission | Implemented |
| YText with UTF-16 boundary conversion | Implemented |
| YMap with Variant↔Any conversion | Implemented |
| Single-byte tagged sync protocol (tags 0/1/2) | Implemented |
| Negotiated DataChannel per peer | Implemented |
| Signaling client reuse (`ws_webrtc_client.gd`) | Implemented |
| `YTextEdit` with virtual-method interception | Implemented (partial; `_delete` + IME pending) |
| Rust relay + coturn bundle | Implemented |
| YArray | Ported in `experiments/exp5_perf/`; ready to copy into the addon |
| YUndoManager (with origin scoping) | Not started |
| YAwareness + presence channel | Minimal codec in `experiments/exp5_perf/`; full version needs dirty-tracking + `set_broadcast_strategy` |
| UpdateChunk framing (tag 3) | Not started |
| Reentrancy guard layer 3 | Not started |
| `YBinder` helper | Partial (`y_map_controls.gd`) |
| `YPoller` autoload | Prototyped in `experiments/exp5_perf/` (~40-line Node); not yet in the addon |
| `YRemoteCursors` overlay | Not started |
| Snapshot / compaction cycle | Not started |
| Mechanical IME test suite | Implemented in `experiments/exp6_ime/` (41 tests, 81 assertions); port under `tests/unit/` when the addon crate lands |
| Multi-platform build pipeline | Linux + WASM only |
