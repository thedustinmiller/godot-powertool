---
name: godot-powertool
description: |
  Godot development assistant — GDScript expertise, scene/script generation patterns, and API reference.
  TRIGGER when: working in a Godot project (any directory with project.godot), writing GDScript (.gd files), editing .tscn scenes, or using Godot MCP tools.
  DO NOT TRIGGER when: the task has nothing to do with Godot or game development.
---

# Godot Powertool Skill

You MUST load this skill when working with Godot projects, GDScript, or Godot MCP tools. If a project.godot exists in the working directory (or any parent), or if the task involves .gd/.tscn files, load this skill immediately.

All files below are in `${CLAUDE_SKILL_DIR}/`. Load progressively — read each file when its phase begins, not upfront.

| File | Purpose | When to read |
|------|---------|--------------|
| `quirks.md` | Known Godot gotchas and workarounds | Before writing any code |
| `gdscript.md` | GDScript syntax reference | Before writing any code |
| `scene-generation.md` | Building `.tscn` files via headless GDScript builders | Targets include `.tscn` |
| `script-generation.md` | Writing runtime `.gd` scripts for node behavior | Targets include `.gd` |
| `coordination.md` | Ordering scene + script generation | Targets include both `.tscn` and `.gd` |
| `doc_api/_common.md` | Index of ~128 common Godot classes (one-line each) | Need API ref; scan to find class names |
| `doc_api/_other.md` | Index of ~732 remaining Godot classes | Need API ref; class isn't in `_common.md` |
| `doc_api/{ClassName}.md` | Full API reference for a single Godot class | Need API ref; look up specific class |

## MCP Tools

If the Godot MCP server is available, use it for all Godot operations. The MCP server provides two modes:

**Editor mode** (preferred): When the Godot editor is running with the PowerTool addon enabled, MCP tools connect via WebSocket for instant operations:
- **Scene tree**: `create_node`, `delete_node`, `update_node_property`, `get_node_properties`, `list_nodes`
- **Scenes**: `create_scene`, `open_scene`, `save_scene`, `get_current_scene`, `get_scene_structure`
- **Scripts**: `create_script_editor`, `edit_script_editor`, `get_script_editor`
- **Editor**: `get_editor_state`, `get_selected_node`, `execute_editor_script`
- **Screenshots**: `take_screenshot` — captures the running game viewport (via EditorDebuggerPlugin) or the editor viewport

**Headless mode** (fallback): When the editor is not running, tools fall back to spawning headless Godot processes. Slower but works in CI.

To play a scene and take screenshots of the running game, use `execute_editor_script` to call `play_main_scene()`, then `take_screenshot` (auto-detects running game).

## API Reference Lookup

1. Need a class? Load `doc_api/_common.md` first (128 most-used classes).
2. Not there? Check `doc_api/_other.md`.
3. Found it? Load `doc_api/{ClassName}.md` for full props/methods/signals/enums.

## Key Patterns

- **Import assets** before using them: `timeout 60 godot --headless --import`
- **Validate** after writing code: `timeout 60 godot --headless --quit`
- **Scene builders** are headless GDScript scripts that produce `.tscn` files (see `scene-generation.md`)
- **Runtime scripts** are `.gd` files that attach to nodes (see `script-generation.md`)
- When both are needed, **generate scenes first**, then scripts (scenes create nodes that scripts attach to)
