---
name: godot-powertool
description: |
  Godot Engine and GDScript development reference. Use when working with Godot projects, writing or editing GDScript (.gd files), editing .tscn scenes, or using Godot MCP tools. Provides GDScript syntax, type system, scene/script generation patterns, known quirks, and Godot API reference.
  DO NOT USE when the task has nothing to do with Godot or game development.
---

# Godot Powertool Skill

You MUST load this skill when working with Godot projects, GDScript, or Godot MCP tools. If a project.godot exists in the working directory (or any parent), or if the task involves .gd/.tscn files, load this skill immediately.

All files below are in `${CLAUDE_SKILL_DIR}/`. Load progressively — read each file when its phase begins, not upfront.

| File | Purpose | When to read |
|------|---------|--------------|
| `gdscript.md` | GDScript syntax, types, and runtime patterns | Before writing any GDScript |
| `quirks.md` | Known Godot/GDScript gotchas and workarounds | Before writing any code |
| `gdextension.md` | Native extension loading, parse-time pitfalls, optional addon pattern | Working on a `.gdextension`, gdext/godot-cpp code, or an optional native addon |
| `doc_api/_common.md` | Index of ~128 common Godot classes (one-line each) | Need API ref; scan to find class names |
| `doc_api/_other.md` | Index of ~732 remaining Godot classes | Need API ref; class isn't in `_common.md` |
| `doc_api/{ClassName}.md` | Full API reference for a single Godot class | Need API ref; look up specific class |

Scene and script *creation* now goes through the Godot MCP tools (live editor) and direct `.gd` / `.tscn` file writes (headless), not through bundled scene-builder scripts. The "MCP Tools" section below lists the relevant tools.

## Project Setup (xtask)

Powertool projects use `cargo xtask` for setup and management. Key commands:

| Command | Purpose |
|---------|---------|
| `cargo xtask init` | First-time setup — downloads Godot, builds MCP + LSP bridge, installs skill |
| `cargo xtask setup` | Re-setup tooling (Godot, MCP, LSP, skill, docs) |
| `cargo xtask editor` | Open the Godot editor |
| `cargo xtask run` | Run the game |
| `cargo xtask test` | Run all tests (Rust + GUT) |
| `cargo xtask mcp run` | Run the MCP server on stdio |
| `cargo xtask lsp-bridge run` | Run the GDScript LSP bridge on stdio |

If setup hasn't been run, start with `cargo xtask init` or `cargo xtask setup`.

## Editor-First Workflow

**Always launch the Godot editor before starting development work.** The editor enables the full MCP toolset:

1. **Launch the editor**: Use `launch_editor` MCP tool (or `cargo xtask editor`). The MCP server will auto-connect via WebSocket.
2. **With the editor running**, you get:
   - Live scene tree manipulation (`create_node`, `update_node_property`, etc.)
   - Run scenes with `run_scene` (uses the editor's debugger for error capture)
   - Take screenshots of the running game with `take_screenshot`
   - GDScript LSP diagnostics (real-time error detection as you edit .gd files)
   - Editor state inspection with `get_editor_state`, `get_selected_node`

**Without the editor**, you are limited to:
- Direct file writes for .gd and .tscn files
- `run_scene_standalone` for running scenes (no debugger, no screenshots)
- `get_editor_log` to read Godot's log file after crashes
- Headless validation: `timeout 30 godot --headless --path <dir> --quit`

**If the editor is not running, launch it.** The overhead is minimal and the tooling gain is significant.

## MCP Tools

If the Godot MCP server is available, use it for all Godot operations:

**Editor mode** (preferred — requires editor running):
- **Scene tree**: `create_node`, `delete_node`, `update_node_property`, `get_node_properties`, `list_nodes`
- **Scenes**: `create_scene`, `open_scene`, `close_scene`, `save_scene`, `reload_scene_from_disk`, `get_current_scene`, `get_scene_structure`
- **Scripts**: `create_script_editor`, `edit_script_editor`, `get_script_editor` (use for incremental edits; for bulk/greenfield scripts, direct file writes are faster)
- **Editor**: `get_editor_state`, `get_selected_node`, `execute_editor_script`
- **Run/Debug**: `run_scene` / `stop_scene`, `take_screenshot` (use `pause_first: true` for CPU-heavy scenes), `get_debug_output`
- **Logs**: `get_editor_log` — read Godot's log file for errors not captured by get_debug_output

**Headless mode** (fallback — no editor needed):
- `run_scene_standalone` / `stop_scene_standalone` — run scenes outside the editor
- `get_debug_output` — captured stdout/stderr from standalone process
- `create_scene` (headless), `get_godot_version`, `get_project_info`, `list_projects`

## Scene Workflow Rules

The Godot editor keeps an **in-memory copy** of every open scene. If you write
to a `.tscn` file directly while it is open in the editor, the editor's copy
diverges from disk and the next `save_scene` will overwrite your file edits.
The editor also pops up a "scene was modified externally, reload from disk?"
dialog that blocks further MCP commands until dismissed.

To stay in sync, follow these rules:

1. **After writing to a `.tscn` file with the Edit/Write tools, call
   `reload_scene_from_disk(path)` if that scene is currently open.** Otherwise
   the in-editor copy will overwrite your changes on next save and the
   "reload from disk?" popup will hang the editor.
2. **Close scenes when you finish a task that opened them.** Use `close_scene`
   on the active scene tab. Long-lived open scenes are the main source of
   editor/agent confusion, especially across unrelated tasks.
3. Prefer the live MCP tools (`update_node_property`, `add_node`, etc.) over
   direct `.tscn` writes when the editor is running. They go through the
   editor's data model so no desync is possible. Direct `.tscn` writes are
   for headless mode or bulk edits the live tools can't express.

If the MCP server starts prepending an `⚠️ WARNING: N Godot processes match…`
banner to its responses, **stop and resolve the duplicates first**. Multiple
editors confuse the agent because the MCP only talks to one of them — the
other(s) silently keep diverging copies of every scene.

## Screenshots

To capture a screenshot of the running game:
1. Ensure the editor is running (`launch_editor`)
2. Run the scene (`run_scene`)
3. Take the screenshot (`take_screenshot`)

For CPU-heavy scenes where `_process()` starves the debugger channel, use `take_screenshot` with `pause_first: true` — this pauses the game tree before capture and resumes after.

## API Reference Lookup

1. Need a class? Load `doc_api/_common.md` first (128 most-used classes).
2. Not there? Check `doc_api/_other.md`.
3. Found it? Load `doc_api/{ClassName}.md` for full props/methods/signals/enums.

## Key Patterns

- **Launch editor first**: `launch_editor` — enables full MCP toolset, LSP diagnostics, and screenshots
- **Import assets** before using them: `timeout 60 godot --headless --import`
- **Validate** after writing code: `timeout 30 godot --headless --path <project_dir> --quit` — exit 0 means all scripts parsed and scenes loaded; non-zero or timeout means errors in stderr (parse errors, missing resources, circular deps)
- When creating scenes and scripts together, do scenes first so the nodes exist for the scripts to reference. Match the script's `extends` to the node it attaches to, and connect signals in the script's `_ready()` (not at build time)
