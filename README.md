# Godot Powertool

Unified template, Rust GDExtension, docs tool, and MCP/Skill bundle. 

## Quick Start

```bash
# Setup; download Godot and GUT testing addon
cargo xtask setup
# or pick explicitly instead of using the setup prompt
cargo xtask setup --agent 7

# Open Godot editor
cargo xtask editor

# Run the application
cargo xtask run

# Run tests
cargo xtask test
```
## About

GDScript is inspired by Python but is NOT Python. It's also relatively low resource compared to Python and the pace of Godot 4+ changes has been quite rapid. So anyone using AI tools to help with Godot projects has run into issues with LLMs writing close, but not quite, script. Additionally, models seem to prefer procedural scene building over .tscn and have limited ability to 'see' what's being done. All together it's hard for them to iterate productively. This toolkit hopes to help with that, while also providing convenience tools for humans as well.

This project was originally a template that gave a quick start for using Rust as a GDExtension and exporting a Godot app using it to the web via WASM. In addition, laziness led me to adding a suite of xtask convenience scripts. Once those were there, adding MCP and other tooling was too tempting.

## Workspace

| Crate | Type | Purpose                                                                 |
|-------|------|-------------------------------------------------------------------------|
| `common` | lib | Shared platform detection, Godot CLI wrappers, config, project scanning |
| `xtask` | bin | Convenience scripts for humans (`cargo xtask <command>`)                |
| `mcp` | bin | MCP server for AI agents (30 tools over stdio + WebSocket)              |
| `docs` | bin | Godot docs -> markdown generator                                        |
| `lsp-bridge` | bin | GDScript LSP bridge for editor integrations |
| `addons/sample_extension/rust` | lib | Optional Rust GDExtension example (disabled by default; see below) |


| Directory | Purpose                                               |
|-----------|-------------------------------------------------------|
| `godot/` | Godot 4.6 project template (scenes, scripts, tests)   |
| `addons/powertool/` | EditorPlugin addon (WebSocket server, live editor tools) |
| `addons/sample_extension/` | Optional Rust GDExtension addon (gdextension manifest, plugin, Rust source) |
| `web/` | Optional Vite frontend + Playwright tests             |
| `skill/` | Agent knowledge base (GDScript ref, quirks, patterns) |
| `agent_templates/` | Starter `AGENTS.md` and `CLAUDE.md` files copied during agent setup |
| `scripts/` | GDScript helpers (MCP operations) |

## Configuration

All versions are managed in `template.toml`:

```toml
[versions]
godot = "4.6.2"
gut = "9.6.0"
mcp = "0.1.0"
skill = "0.1.0"
docs = "4.6.2"
```

## CLI Reference

```bash
cargo xtask setup              # Download Godot + GUT, initialize project, prompt for agent setup
cargo xtask setup --agent 7       # Configure Claude Code + Codex CLI + Cursor
cargo xtask setup --agent 3       # Configure Claude Code + Codex CLI
cargo xtask setup --agent cursor  # Configure only Cursor instead of prompting
cargo xtask build              # Build Rust GDExtension (if enabled)
cargo xtask run                # Run the game
cargo xtask editor             # Open Godot editor
cargo xtask dev                # Build + run
cargo xtask test               # Run all tests (Rust + GUT)
cargo xtask doctor             # Check project health
cargo xtask fmt                # Format Rust code
cargo xtask lint               # Run clippy
cargo xtask ci                 # fmt + lint + test

# Web (optional)
cargo xtask web setup          # Check prerequisites
cargo xtask web export         # Export Godot project for web
cargo xtask web dev            # Vite dev server on :3000
cargo xtask web test           # Playwright tests

# Agent tooling
cargo xtask skill install      # Install skill files for Claude Code
cargo xtask skill install -t codex    # ...or Codex CLI
cargo xtask skill install -t generic  # ...or generic skills/ dir
cargo xtask skill install -t /custom/path
cargo xtask skill update       # Regenerate + reinstall
cargo xtask skill remove       # Remove installed skill files

cargo xtask mcp run            # Launch MCP server on stdio
cargo xtask mcp install claude # Print setup instructions for Claude Code
cargo xtask mcp install codex  # Print setup instructions for Codex CLI
cargo xtask mcp install cursor # Print config for Cursor
```

Setup also copies root instruction files for the selected agent when they do
not already exist: `AGENTS.md` for Codex CLI and Cursor, and `CLAUDE.md` for
Claude Code. The copied `AGENTS.md` is intentionally a starter file; keep it
updated as the generated project becomes a real game or app.

## MCP Server

The MCP server provides 30 tools for AI agents to interact with Godot. When the editor is running with the PowerTool addon, tools connect via WebSocket for instant operations. Without the editor, headless Godot fallback is used.

### Headless tools (always available)

| Tool | Description |
|------|-------------|
| `get_godot_version` | Get installed Godot version |
| `launch_editor` / `stop_editor` | Open/close the Godot editor |
| `run_scene_standalone` / `stop_scene_standalone` | Run/stop a scene as a standalone process (outside editor) |
| `get_debug_output` | Read stdout/stderr from running scene |
| `list_projects` | Find Godot projects in a directory |
| `get_project_info` | Get project metadata (scenes, scripts, etc.) |
| `create_scene` / `add_node` / `save_scene` | Scene file manipulation (headless) |
| `load_sprite` | Load a texture into a sprite node |
| `export_mesh_library` | Export 3D scene as MeshLibrary |
| `get_uid` / `update_project_uids` | UID management (Godot 4.4+) |

### Editor tools (require PowerTool addon)

| Tool | Description |
|------|-------------|
| `create_node` / `delete_node` | Add/remove nodes in the live scene tree |
| `update_node_property` | Set properties on nodes with undo/redo |
| `get_node_properties` / `list_nodes` | Inspect the live scene tree |
| `open_scene` / `get_current_scene` | Scene navigation |
| `get_scene_structure` | Full recursive tree dump |
| `create_script_editor` / `edit_script_editor` / `get_script_editor` | Script CRUD via editor |
| `get_editor_state` / `get_selected_node` | Editor state and selection |
| `execute_editor_script` | Run arbitrary GDScript in the editor |
| `run_scene` / `stop_scene` | Run/stop a scene via the editor (preferred) |
| `take_screenshot` | Capture running game (via debugger) or editor viewport |

Multiple agents can connect simultaneously. Mutating commands use transparent per-resource locking with 5s timeout — crashed agents' locks expire automatically.

### Setup

```bash
# Build the server
cargo build -p powertool-mcp --release

# Add to Claude Code
claude mcp add godot -- ./target/release/powertool-mcp

# Add to Codex CLI
codex mcp add godot -- ./target/release/powertool-mcp

# Add to Cursor project config (.cursor/mcp.json)
{
  "mcpServers": {
    "godot": {
      "type": "stdio",
      "command": "./target/release/powertool-mcp",
      "args": []
    }
  }
}

# Or set GODOT_PATH if Godot isn't on your PATH
GODOT_PATH=/path/to/godot claude mcp add godot -- ./target/release/powertool-mcp
```

### PowerTool Addon

The editor addon lives in `addons/powertool/`. To install in your Godot project:

1. Copy (or symlink) `addons/powertool/` into your project's `addons/` directory
2. Open the project in Godot
3. Project → Project Settings → Plugins → Enable "PowerTool"

The addon starts a WebSocket server on `127.0.0.1:6550` (configurable via `POWERTOOL_PORT` env var). It also registers a game-side autoload (`PowerToolGame`) that enables screenshots and scene inspection of running games via the Godot debugger connection.

## API Doc Generator

Generates LLM-friendly Markdown from Godot's XML class documentation, pinned to the Godot version in `template.toml`.

```bash
# Generate docs (fetches Godot XML source automatically)
cargo run -p powertool-godot-docs -- generate

# Generate for a specific version
cargo run -p powertool-godot-docs -- generate --version 4.5.0

# Clean generated files
cargo run -p powertool-godot-docs -- clean
```

Output goes to `doc_api/` — per-class `.md` files plus two indexes:
- `_common.md` — 128 most-used classes
- `_other.md` — everything else

These are included automatically when you run `cargo xtask skill install`.

## Agent Skill

The `skill/` directory contains a knowledge base for AI coding assistants:

- **SKILL.md** — manifest with progressive loading instructions
- **gdscript.md** — GDScript language reference and runtime patterns
- **quirks.md** — documented Godot/GDScript gotchas
- **gdextension.md** — native extension loading, parse-time pitfalls, optional addon pattern
- **doc_api/** — generated API reference (860+ classes)

Install into your project for your agent of choice:

```bash
cargo xtask skill install                    # Claude Code (.claude/skills/)
cargo xtask skill install --target codex     # Codex CLI (.agents/skills/)
cargo xtask skill install --target generic   # Plain (skills/)
```

Cursor setup writes a project rule at `.cursor/rules/godot-powertool.mdc`
that references this knowledge base, and writes MCP configuration to
`.cursor/mcp.json`.

Codex and Cursor setup copy `agent_templates/AGENTS.md` to the project root
when no `AGENTS.md` exists. Claude Code setup copies
`agent_templates/CLAUDE.md` as a placeholder for Claude-specific guidance.

## Rust GDExtension (Optional)

The project ships with a **`addons/sample_extension/`** addon — a complete,
self-contained Rust GDExtension example built on
[godot-rust/gdext](https://github.com/godot-rust/gdext) 0.5. It is disabled by
default. The base `addons/powertool/` addon does not depend on it; deleting the
`addons/sample_extension/` directory leaves the rest of the project running
unchanged.

The design follows a "binary-or-slow, never binary-or-bust" pattern: the
addon's own `plugin.gd` defensively probes `ClassDB.class_exists(...)` before
using any native class, so a missing or platform-mismatched binary produces a
warning instead of a crash. See
`~/Desktop/sho/docs/plans/native-split.md` for the full design rationale.

### Enable

1. In root `Cargo.toml`, uncomment `"addons/sample_extension/rust"` in the
   workspace `members` list.
2. Run `cargo xtask build` to compile and stage the binary into
   `godot/addons/sample_extension/bin/<platform>/`.
3. Open the project in Godot and enable
   **Project Settings → Plugins → "Sample Rust Extension"**.

From GDScript, the example registers `SampleGreeter` (a `RefCounted`):

```gdscript
var greeter := SampleGreeter.new()
print(greeter.greet("World"))   # "Hello, World!"
print(greeter.fibonacci(40))    # 102334155
```

### Layout

```
addons/sample_extension/
├── plugin.cfg
├── plugin.gd                    # @tool EditorPlugin, defensive ClassDB probe
├── sample_extension.gdextension # Library manifest (per-platform paths)
├── bin/                         # Built binaries (gitignored, populated by xtask build)
└── rust/
    ├── Cargo.toml
    └── src/lib.rs               # `SampleGreeter` and the gdextension entry symbol
```

## Threading

Default builds are **multithreaded** end-to-end. Native desktop/mobile use OS threads as usual; web/WASM uses `pthread` + `+atomics` and Godot's threaded runtime, which requires the page to be served with `Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy: require-corp` (so `SharedArrayBuffer` is available). The Vite dev/preview servers in `web/` already set those headers.

If you need to ship a build for environments that can't satisfy COOP+COEP (some shared hosts, embeds inside other sites), there's a single-threaded fallback. Five knobs control threading; flip all of them together when changing modes:

| Knob | File | Threaded (default) | Single-threaded |
|------|------|--------------------|-----------------|
| xtask build flag | `cargo xtask build --web ...` | `--web` (or `--both` for both variants) | `--web --nothreads` |
| Cargo feature | `addons/sample_extension/rust/Cargo.toml` | default features | `--features nothreads` |
| RUSTFLAGS | `xtask/src/main.rs` (`WASM_RUSTFLAGS_*`) | `WASM_RUSTFLAGS_BASE + WASM_RUSTFLAGS_THREADS` | `WASM_RUSTFLAGS_BASE` only |
| Godot export | `godot/export_presets.cfg` | `variant/thread_support=true` | `variant/thread_support=false` |
| GDExtension manifest | `addons/sample_extension/sample_extension.gdextension` | already lists both `web.*.threads.wasm32` and `web.*.wasm32` — Godot picks based on the export setting | (no change needed) |

The xtask flow handles the first three for you — `--web` runs the threaded build with the right flags, `--web --nothreads` flips to the single-threaded variant, and `--web --both` produces both files so you can toggle the export preset without rebuilding. The export preset and the manifest are part of the Godot project itself, so they're not driven from the CLI; flip them in the editor or by hand. After changing `variant/thread_support`, reload the Godot project before re-exporting.

The runtime knobs `threads/emscripten_pool_size` and `threads/godot_pool_size` in `export_presets.cfg` only matter when threaded — they size the worker pools for emscripten and Godot respectively.

## License

MIT OR Apache-2.0

## Acknowledgments

This project is inspired by and heavily based on the following MIT licensed projects:

- [godogen](https://github.com/htdt/godogen): Skill design, quirk docs, Godot doc preparation, scene building
- [godot-mcp](https://github.com/Coding-Solo/godot-mcp): MCP server 
- [godot-mcp-screenshot](https://github.com/tylerhaar7/godot-mcp-screenshot): Adding screenshot functionality to godot-mcp

See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for full attribution and license text.
