# Godot Powertool

Unified template, Rust GDExtension, docs tool, and MCP/Skill bundle. 

## Quick Start

```bash
# Setup; download Godot and GUT testing addon
cargo xtask setup

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


| Directory | Purpose                                               |
|-----------|-------------------------------------------------------|
| `godot/` | Godot 4.6 project template (scenes, scripts, tests)   |
| `extension/` | Optional Rust GDExtension (disabled by default)       |
| `web/` | Optional Vite frontend + Playwright tests             |
| `skill/` | Agent knowledge base (GDScript ref, quirks, patterns) |
| `addons/powertool/` | EditorPlugin addon (WebSocket server, live editor tools) |
| `scripts/` | GDScript helpers (MCP operations, screenshot capture) |

## Configuration

All versions are managed in `template.toml`:

```toml
[versions]
godot = "4.6.1"
gut = "9.6.0"
mcp = "0.1.0"
skill = "0.1.0"
docs = "4.6.1"
```

## CLI Reference

```bash
cargo xtask setup              # Download Godot + GUT, initialize project
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
cargo xtask mcp install cursor # Print config for Cursor
```

## MCP Server

The MCP server provides 30 tools for AI agents to interact with Godot. When the editor is running with the PowerTool addon, tools connect via WebSocket for instant operations. Without the editor, headless Godot fallback is used.

### Headless tools (always available)

| Tool | Description |
|------|-------------|
| `get_godot_version` | Get installed Godot version |
| `launch_editor` | Open the Godot editor |
| `run_project` / `stop_project` | Run/stop a project in debug mode |
| `get_debug_output` | Read stdout/stderr from running project |
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
| `take_screenshot` | Capture running game (via debugger) or editor viewport |

Multiple agents can connect simultaneously. Mutating commands use transparent per-resource locking with 5s timeout — crashed agents' locks expire automatically.

### Setup

```bash
# Build the server
cargo build -p powertool-mcp --release

# Add to Claude Code
claude mcp add godot -- ./target/release/powertool-mcp

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
cargo run -p powertool-docs -- generate

# Generate for a specific version
cargo run -p powertool-docs -- generate --version 4.5.0

# Clean generated files
cargo run -p powertool-docs -- clean
```

Output goes to `doc_api/` — per-class `.md` files plus two indexes:
- `_common.md` — 128 most-used classes
- `_other.md` — everything else

These are included automatically when you run `cargo xtask skill install`.

## Agent Skill

The `skill/` directory contains a knowledge base for AI coding assistants:

- **SKILL.md** — manifest with progressive loading instructions
- **gdscript.md** — GDScript language reference
- **quirks.md** — 18+ documented Godot engine gotchas
- **scene-generation.md** — patterns for programmatic `.tscn` creation
- **script-generation.md** — runtime `.gd` script patterns
- **coordination.md** — ordering scene + script generation
- **doc_api/** — generated API reference (860+ classes)

Install into your project for your agent of choice:

```bash
cargo xtask skill install                    # Claude Code (.claude/skills/)
cargo xtask skill install --target codex     # Codex CLI (.agents/skills/)
cargo xtask skill install --target generic   # Plain (skills/)
```

## Rust GDExtension (Optional)

Disabled by default. To enable:

1. In root `Cargo.toml`, change `members` to include `"extension"`
2. In `godot/project.godot`, enable the extension plugin
3. Run `cargo xtask build`

See the extension directory for example Rust classes exposed to Godot.

## License

MIT OR Apache-2.0

## Acknowledgments

This project is inspired by and heavily based on the following MIT licensed projects:

- [godogen](https://github.com/htdt/godogen): Skill design, quirk docs, Godot doc preparation, scene building
- [godot-mcp](https://github.com/Coding-Solo/godot-mcp): MCP server 
- [godot-mcp-screenshot](https://github.com/tylerhaar7/godot-mcp-screenshot): Adding screenshot functionality to godot-mcp

See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for full attribution and license text.
