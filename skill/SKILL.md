---
name: godot-powertool
description: |
  Godot development assistant — GDScript expertise, scene/script generation patterns, and API reference.
---

# Godot Powertool Skill

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
