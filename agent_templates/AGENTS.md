# Project Agent Instructions

This project was created from the Godot PowerTool template. Treat Godot PowerTool as a starting point and tool bundle for building a different Godot application, not as the end product itself. Prefer decisions that serve the application being built in this repository.

Keep this `AGENTS.md` current. When you learn project-specific architecture, workflows, test commands, Godot scene conventions, export requirements, or agent/tooling setup that future agents should know, update this file proactively as part of the same change. Do not let it remain a generic template note once the project starts to diverge.

## Working Principles

- Read the nearby code, scenes, resources, and docs before making broad changes.
- Preserve user work and existing project-specific behavior. Do not revert unrelated changes.
- Prefer Godot scene/resource edits and the PowerTool editor tools over procedural scene reconstruction when practical.
- Keep generated or copied template files aligned with the real project as it evolves.
- Use the smallest focused change that satisfies the task, then verify it with the relevant command or Godot workflow.
- If a change affects runtime behavior, scripts, scenes, exports, or agent setup, update docs or this file when the next agent would otherwise have to rediscover the information.

## Godot PowerTool Context

The PowerTool MCP server gives agents and humans Godot-aware operations over stdio, with live editor support through the `addons/powertool` plugin. Prefer those tools for scene inspection, screenshots, live node edits, running scenes, and Godot project metadata when they are available.

The `skill/` directory contains Godot-focused guidance:

- `skill/SKILL.md` - progressive loading instructions.
- `skill/gdscript.md` - GDScript language and runtime reference.
- `skill/quirks.md` - known Godot and GDScript gotchas.
- `skill/gdextension.md` - native extension guidance.
- `doc_api/` - generated Godot API docs when available.

Load the relevant skill files before nontrivial Godot or GDScript work.

## Common Commands

Use these commands from the repository root unless a project-specific replacement has been documented here:

```bash
cargo xtask setup
cargo xtask editor
cargo xtask run
cargo xtask test
cargo xtask fmt
cargo xtask lint
```

For MCP setup:

```bash
cargo xtask mcp install codex
cargo xtask mcp install cursor
```

For generated Godot API docs:

```bash
cargo run -p powertool-godot-docs -- generate
```

## Update This File

Revise this file when:

- The actual game/app architecture becomes clearer.
- New scenes, autoloads, addons, tools, or export targets become central.
- The test, lint, build, or release commands change.
- A recurring Godot, GDScript, GDExtension, or asset pipeline issue is discovered.
- Agent setup changes, such as MCP server names, skill locations, Cursor rules, or editor requirements.

Prefer concrete instructions and commands over broad preferences. Remove template-only notes once they stop helping.
