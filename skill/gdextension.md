# GDExtension & Loading

Read this when working on a GDExtension (gdext, godot-cpp), shipping an optional native addon, or debugging "Identifier X not declared" / "GDExtension dynamic library not found" failures.

## The single insight that explains most of this

Godot's startup pipeline has no recovery point between **load extensions** and **parse scripts**. The order is roughly:

1. Engine init
2. Recursive `res://` scan for `*.gdextension` files → `OS.open_dynamic_library()` for each → call the extension entry symbol, which registers classes via `ClassDB`
3. Parse and compile every script with `class_name`
4. Autoload `_init`, then scenes, then `EditorPlugin._enter_tree` (editor) or `Node._ready` (runtime)

The parser is the gatekeeper at step 3. By the time anything you can write — a `_ready()`, an autoload, an `EditorPlugin._enter_tree` — runs, every script in the project has already had to parse cleanly. If a script parses-references a class that wasn't registered in step 2, that script fails to load and nothing downstream rescues it.

Most "optional GDExtension" patterns fail because they try to gate the *call* with a runtime flag while still naming the class as a typed identifier. The parser walked over the typed name before any flag could be checked.

Five rules of thumb fall out:

- **Identifier resolution is parse-time, not runtime.** `class_name` symbols are resolved before any branch is evaluated. Branch reachability does not save you.
- **No two-pass parse.** A plugin cannot "register the class first" because plugin hooks fire after the parse pass.
- **The `.gdextension` file is the trigger, not the binary.** If the file is in `res://`, Godot will try to load the binary. There is no "best-effort" mode.
- **`reloadable = true`** is for hot-rebuild during dev, not fault tolerance for a missing binary.
- **gdext binds engine classes only.** `#[class(base = ...)]` accepts engine types (`Object`, `RefCounted`, `Node`, `Resource`, `Control`, ...). User `class_name` types live in the GDScript layer; gdext cannot subclass them.

## How to ship an optional GDExtension

The presence of a `.gdextension` file is what triggers loading, so the unit of "optional" must be at the **addon-directory granularity**, not a runtime flag.

```
addons/
├── my_main_addon/         # always present
└── my_native_addon/       # user opts in by installing this directory
    ├── plugin.cfg
    ├── plugin.gd          # @tool, defensively probes ClassDB
    ├── my_native.gdextension
    ├── bin/<platform>/...
    └── rust/ (or src/)
```

Consumers in the main addon must avoid parse-visible references to native classes:

```gdscript
# Avoid — fails to parse if MyNative isn't registered:
var x := MyNative.new()
MyNative.do_thing()

# Prefer — class name is a string, never enters the parser's symbol table:
var x: Object = ClassDB.instantiate(&"MyNative")
if x:
    x.call(&"do_thing")
```

The addon's own `plugin.gd` should probe before using anything:

```gdscript
@tool
extends EditorPlugin

func _enter_tree() -> void:
    if not ClassDB.class_exists(&"MyNative"):
        push_warning("my_native_addon: binary missing — addon stays inert")
        return
    # safe to use the native class from here on
```

This is the pattern in `addons/sample_extension/` in this repo.

## Late binding with `Callable`

`Callable(object, &"method_name")` stores an Object reference and a method name as a StringName. Resolution happens at `.call()` time, so the consuming code never names the method as a typed identifier.

```gdscript
class_name MathDispatch extends RefCounted

# Default to a pure-GDScript implementation. No external class_name reference;
# this file parses with or without the native addon.
static var lerp_floats: Callable = Callable(MathDispatch, &"_lerp_floats_gd")

static func _lerp_floats_gd(a: PackedFloat32Array, b: PackedFloat32Array, t: float) -> PackedFloat32Array:
    ...

# In the optional addon's plugin.gd:
func _enter_tree() -> void:
    var native: Object = ClassDB.instantiate(&"NativeMath")
    if native:
        MathDispatch.lerp_floats = Callable(native, &"lerp_packed_floats")
```

Cost: per-invocation Variant boxing — tens of nanoseconds. Cheap for batch calls, expensive for per-element loops. Shape native APIs to take whole `Packed*Array`s in and out, not one element at a time.

## Things that look like they should work, but don't

**`class_name` reference inside an unreachable branch.**

```gdscript
# Parse error even though the runtime guard would skip the line:
if ClassDB.class_exists(&"MyNative"):
    return MyNative.do_thing()  # parser already flagged "MyNative" as undeclared
```

GDScript resolves identifiers like Java/C#, not like Python. There is no flow-sensitive name resolution. Use `Object.call(&"name", ...)` or a `Callable` slot.

**`preload()` of a sometimes-missing resource.**

```gdscript
const X := preload("res://addons/optional/something.tres")  # parse error if file absent
var x := load("res://addons/optional/something.tres")        # null at runtime if absent
```

`preload()` is parse-time, `load()` is runtime. Use `load()` for any soft reference.

**Subclassing a user GDScript class from gdext.**

```rust
#[derive(GodotClass)]
#[class(base = MyGDScriptBase)]   // not bindable — gdext only knows engine classes
struct MyNative { base: Base<MyGDScriptBase> }
```

Use composition or `Callable` injection instead of inheritance across the boundary.

**Hiding a `.gdextension` file by burying it in a deep folder.**

Godot scans `res://` recursively. Path depth doesn't help. Real ways to suppress auto-load:

- Rename the suffix (e.g. `.gdextension.disabled`) — the discovery glob is suffix-sensitive.
- Ship the file in a separate addon directory the user installs only when they want it (the recommended pattern).
- Defer-load via `GDExtensionManager.load_extension(path)` — but only if no parsed script names the extension's classes, since the parse pass has already run by the time you can call it.

## Lifecycle and load-order quirks

- **`reloadable = true`** lets you `cargo build` while the editor is running and pick up the new binary. It does not tolerate a missing binary at boot, and does not re-fire `EditorPlugin._enter_tree` on hot-reload — `Callable`s injected into other scripts will reference stale Object instances after a rebuild. Document that Rust rebuilds may need an editor restart, or expose a manual "re-inject" trigger.
- **Static var init within a file** is deterministic top-to-bottom. **Across files** the order is parse-graph dependent and usually fine, but cross-class static initializers that touch each other can hit "X used before initialization" on cycles. Godot 4.4+ adds `static func _static_init() -> void:` for setup that needs to run after the file's own statics are ready, with inter-class ordering explicitly undefined.
- **Addons enable in the order listed in `project.godot`'s `[editor_plugins]` section**, not filesystem order. If addon B depends on addon A, list A first. Don't rely on alphabetical filenames for ordering — verify the project file.
- **`@tool` propagates through `class_name` references.** A `@tool` chart class that imports a plain dispatch helper for editor preview will see the helper degraded to editor-only stubs. If a class is consumed by `@tool` code at edit time, mark it `@tool` too.
- **`GDExtensionManager`** (`load_extension`, `unload_extension`, `reload_extension`, `is_extension_loaded`, `get_loaded_extensions`) gives runtime control. It pairs with the late-binding patterns above; without them it can't help, because parse already happened.

## Performance: dispatch boundaries box every argument

Variant boxing applies on every cross-boundary call:

- `Object.call(&"method", a, b)`
- `Callable.call(a, b)`
- GDScript → GDExtension method invocation
- GDScript → GDScript when types erase to Variant

`Packed*Array` arguments share the underlying buffer (refcount bump, no copy), so the boxing cost is per-call, not per-element. This is why native APIs in this style accept and return whole arrays — per-element FFI is almost always slower than pure GDScript.

## Distribution note

Godot's Asset Library historically prefers pure-GDScript addons: faster review, broadest compatibility, no per-platform binary packaging. Native addons review more slowly and ship only the platforms baked into the published version. Library authors get the widest reach by publishing a pure-GDScript addon to the Asset Library and shipping the optional native acceleration separately (GitHub releases, your own CDN). Users on a Godot version gdext hasn't caught up to can still install the GDScript half.
