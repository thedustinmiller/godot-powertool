use std::{
    fs,
    io::{self, BufRead},
    path::Path,
    process::{Command, Stdio},
};

use anyhow::{Context, Result, bail};

use super::{cmd_lsp_bridge_install, project_root};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentTarget {
    ClaudeCode,
    Codex,
    Cursor,
}

impl AgentTarget {
    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "cc" | "claude" | "claude-code" | "claude_code" | "claude code" => {
                Some(Self::ClaudeCode)
            }
            "codex" | "codex-cli" | "codex_cli" | "codex cli" => Some(Self::Codex),
            "cursor" => Some(Self::Cursor),
            _ => None,
        }
    }

    fn bit(self) -> u8 {
        match self {
            Self::ClaudeCode => 1,
            Self::Codex => 2,
            Self::Cursor => 4,
        }
    }

    pub(crate) fn skill_target(self) -> Option<&'static str> {
        match self {
            Self::ClaudeCode => Some("claude"),
            Self::Codex => Some("codex"),
            Self::Cursor => None,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex CLI",
            Self::Cursor => "Cursor",
        }
    }

    pub(crate) fn instruction_files(self) -> &'static [&'static str] {
        match self {
            Self::ClaudeCode => &["CLAUDE.md"],
            Self::Codex | Self::Cursor => &["AGENTS.md"],
        }
    }
}

const AGENTS_IN_ORDER: [AgentTarget; 3] = [
    AgentTarget::ClaudeCode,
    AgentTarget::Codex,
    AgentTarget::Cursor,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AgentSelection {
    bits: u8,
}

impl AgentSelection {
    pub(crate) const NONE: Self = Self { bits: 0 };

    fn from_bits(bits: u8) -> Option<Self> {
        if bits <= 7 { Some(Self { bits }) } else { None }
    }

    fn from_target(target: AgentTarget) -> Self {
        Self { bits: target.bit() }
    }

    pub(crate) fn contains(self, target: AgentTarget) -> bool {
        self.bits & target.bit() != 0
    }

    pub(crate) fn is_empty(self) -> bool {
        self.bits == 0
    }

    pub(crate) fn iter(self) -> impl Iterator<Item = AgentTarget> {
        AGENTS_IN_ORDER
            .into_iter()
            .filter(move |a| self.contains(*a))
    }

    pub(crate) fn label(self) -> String {
        if self.is_empty() {
            return "Skip agent setup".to_string();
        }
        self.iter()
            .map(AgentTarget::label)
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub(crate) fn skill_targets(self) -> impl Iterator<Item = &'static str> {
        self.iter().filter_map(AgentTarget::skill_target)
    }
}

impl std::str::FromStr for AgentSelection {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Ok(Self::from_target(AgentTarget::ClaudeCode));
        }
        let lower = trimmed.to_ascii_lowercase();
        if matches!(lower.as_str(), "none" | "skip" | "no" | "0") {
            return Ok(Self::NONE);
        }
        if matches!(lower.as_str(), "all") {
            return Ok(Self { bits: 7 });
        }
        if let Ok(bits) = lower.parse::<u8>() {
            return Self::from_bits(bits)
                .with_context(|| format!("Agent bitmask `{bits}` is out of range. Use 0-7."));
        }

        let mut bits = 0;
        for part in lower
            .split([',', '+', '|'])
            .map(str::trim)
            .filter(|p| !p.is_empty())
        {
            let target = AgentTarget::from_str(part).with_context(|| {
                format!(
                    "Unknown agent target `{part}`. Expected claude, codex, cursor, all, none, or bitmask 0-7."
                )
            })?;
            bits |= target.bit();
        }

        Ok(Self { bits })
    }
}

pub(crate) fn prompt_agent_selection(configured: Option<&str>) -> Result<AgentSelection> {
    if let Some(value) = configured {
        return value.parse();
    }

    println!("Select AI assistants to configure:");
    println!("  1) Claude Code");
    println!("  2) Codex CLI");
    println!("  4) Cursor");
    println!(
        "Use sums to select multiple: 3 = Claude Code + Codex CLI, 5 = Claude Code + Cursor, 7 = all."
    );
    println!("  0) Skip agent setup");

    loop {
        eprint!("Agents [1]: ");
        let mut input = String::new();
        io::stdin().lock().read_line(&mut input)?;
        let trimmed = input.trim();
        let value = if trimmed.is_empty() { "1" } else { trimmed };
        match value.parse() {
            Ok(selection) => return Ok(selection),
            Err(e) => eprintln!("{e}"),
        }
    }
}

fn claude_code_detected() -> bool {
    which::which("claude").is_ok()
}

fn codex_detected() -> bool {
    which::which("codex").is_ok()
}

fn try_register_mcp_with_claude(mcp_bin: &Path) -> Result<bool> {
    if !claude_code_detected() {
        return Ok(false);
    }
    let mcp_path = mcp_bin.to_string_lossy();

    let _ = Command::new("claude")
        .args(["mcp", "remove", "godot"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    let status = Command::new("claude")
        .args(["mcp", "add", "godot", "--", &mcp_path])
        .status()
        .context("Failed to invoke `claude mcp add`")?;

    if !status.success() {
        bail!(
            "`claude mcp add godot -- {mcp_path}` exited with {:?}",
            status.code()
        );
    }
    Ok(true)
}

fn try_register_mcp_with_codex(mcp_bin: &Path) -> Result<bool> {
    if !codex_detected() {
        return Ok(false);
    }
    let mcp_path = mcp_bin.to_string_lossy();

    let _ = Command::new("codex")
        .args(["mcp", "remove", "godot"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    let status = Command::new("codex")
        .args(["mcp", "add", "godot", "--", &mcp_path])
        .status()
        .context("Failed to invoke `codex mcp add`")?;

    if !status.success() {
        bail!(
            "`codex mcp add godot -- {mcp_path}` exited with {:?}",
            status.code()
        );
    }
    Ok(true)
}

fn install_mcp_for_cursor(root: &Path, mcp_bin: &Path) -> Result<bool> {
    let cursor_dir = root.join(".cursor");
    let settings_path = cursor_dir.join("mcp.json");
    let mcp_path = mcp_bin.to_string_lossy();

    let mut settings: serde_json::Value = if settings_path.exists() {
        let contents =
            fs::read_to_string(&settings_path).context("Failed to read .cursor/mcp.json")?;
        serde_json::from_str(&contents).context("Failed to parse .cursor/mcp.json")?
    } else {
        serde_json::json!({})
    };

    let obj = settings
        .as_object_mut()
        .context(".cursor/mcp.json is not an object")?;
    let servers = obj
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}));
    servers
        .as_object_mut()
        .context("mcpServers is not an object")?
        .insert(
            "godot".to_string(),
            serde_json::json!({
                "type": "stdio",
                "command": mcp_path,
                "args": []
            }),
        );

    fs::create_dir_all(&cursor_dir).context("Failed to create .cursor directory")?;
    let formatted =
        serde_json::to_string_pretty(&settings).context("Failed to serialize .cursor/mcp.json")?;
    fs::write(&settings_path, formatted + "\n").context("Failed to write .cursor/mcp.json")?;
    println!("Wrote Cursor MCP config to {}", settings_path.display());
    Ok(true)
}

pub(crate) fn try_register_mcp_for_agent(agent: AgentTarget, mcp_bin: &Path) -> Result<bool> {
    match agent {
        AgentTarget::ClaudeCode => try_register_mcp_with_claude(mcp_bin),
        AgentTarget::Codex => try_register_mcp_with_codex(mcp_bin),
        AgentTarget::Cursor => install_mcp_for_cursor(&project_root()?, mcp_bin),
    }
}

pub(crate) fn try_register_mcp_for_agents(
    selection: AgentSelection,
    mcp_bin: &Path,
) -> Result<bool> {
    let mut registered_any = false;
    for agent in selection.iter() {
        match try_register_mcp_for_agent(agent, mcp_bin) {
            Ok(true) => {
                println!("Registered MCP server with {}", agent.label());
                registered_any = true;
            }
            Ok(false) => {
                println!("Could not auto-register MCP for {}.", agent.label());
            }
            Err(e) => {
                eprintln!(
                    "Warning: auto-registering MCP for {} failed: {e}",
                    agent.label()
                );
                print_mcp_retry(agent, mcp_bin);
            }
        }
    }
    Ok(registered_any)
}

fn install_cursor_rules_at(root: &Path) -> Result<()> {
    let rules_dir = root.join(".cursor").join("rules");
    let rule_path = rules_dir.join("godot-powertool.mdc");
    fs::create_dir_all(&rules_dir).context("Failed to create .cursor/rules directory")?;

    let content = r#"---
description: Godot PowerTool project guidance
globs: **/*.gd, **/*.tscn, **/*.tres, **/*.gdextension, Cargo.toml
alwaysApply: false
---

Use the Godot PowerTool MCP server for live editor operations, scene inspection, screenshots, and Godot-aware project actions when available.

For Godot and GDScript work, load the project knowledge base before making broad changes:

@skill/SKILL.md
@skill/gdscript.md
@skill/quirks.md
@skill/gdextension.md

Prefer existing scene resources and editor-backed changes over rebuilding scenes procedurally in scripts. Validate GDScript with the project tooling when edits touch runtime behavior.
"#;

    fs::write(&rule_path, content).context("Failed to write Cursor rule")?;
    println!("Wrote Cursor project rule to {}", rule_path.display());
    Ok(())
}

pub(crate) fn install_agent_editor_config(agent: AgentTarget) -> Result<()> {
    match agent {
        AgentTarget::ClaudeCode => {
            if claude_code_detected() {
                cmd_lsp_bridge_install("claude")
            } else {
                println!("Claude Code CLI not found on PATH - skipping LSP auto-install.");
                Ok(())
            }
        }
        AgentTarget::Cursor => install_cursor_rules_at(&project_root()?),
        AgentTarget::Codex => Ok(()),
    }
}

pub(crate) fn install_agent_editor_configs(selection: AgentSelection) -> Result<bool> {
    let mut installed_any = false;
    for agent in selection.iter() {
        match install_agent_editor_config(agent) {
            Ok(()) => installed_any = true,
            Err(e) => {
                eprintln!(
                    "Warning: editor config install for {} failed: {e}",
                    agent.label()
                );
                print_lsp_retry(agent);
            }
        }
    }
    Ok(installed_any)
}

pub(crate) fn print_mcp_retry(agent: AgentTarget, mcp_bin: &Path) {
    match agent {
        AgentTarget::ClaudeCode => println!(
            "  claude mcp add godot -- {}  # Register MCP server with Claude Code",
            mcp_bin.display()
        ),
        AgentTarget::Codex => println!(
            "  codex mcp add godot -- {}  # Register MCP server with Codex CLI",
            mcp_bin.display()
        ),
        AgentTarget::Cursor => {
            println!("  cargo xtask mcp install cursor  # Print Cursor MCP config");
        }
    }
}

pub(crate) fn print_lsp_retry(agent: AgentTarget) {
    match agent {
        AgentTarget::ClaudeCode => {
            println!("  cargo xtask lsp-bridge install claude  # Configure GDScript LSP");
        }
        AgentTarget::Cursor => {
            println!("  Re-run `cargo xtask setup --agent cursor` to regenerate Cursor rules");
        }
        AgentTarget::Codex => {}
    }
}

pub(crate) fn print_mcp_retries(selection: AgentSelection, mcp_bin: &Path) {
    for agent in selection.iter() {
        print_mcp_retry(agent, mcp_bin);
    }
}

pub(crate) fn print_lsp_retries(selection: AgentSelection) {
    for agent in selection.iter() {
        print_lsp_retry(agent);
    }
}

fn copy_agent_instruction_files(root: &Path, source_dir: &Path, agent: AgentTarget) -> Result<()> {
    for file in agent.instruction_files() {
        let source = source_dir.join(file);
        let dest = root.join(file);

        if !source.exists() {
            bail!(
                "Agent instruction template not found at {}",
                source.display()
            );
        }
        if dest.exists() {
            println!("Keeping existing {}", dest.display());
            continue;
        }

        fs::copy(&source, &dest).with_context(|| {
            format!("Failed to copy {} -> {}", source.display(), dest.display())
        })?;
        println!("Copied {}", dest.display());
    }

    Ok(())
}

pub(crate) fn install_agent_instruction_files(selection: AgentSelection) -> Result<()> {
    if selection.is_empty() {
        return Ok(());
    }

    let root = project_root()?;
    let source_dir = root.join("agent_templates");
    if !source_dir.exists() {
        bail!(
            "Agent instruction template directory not found at {}",
            source_dir.display()
        );
    }

    println!("\n=== Installing agent instruction files ===\n");
    for agent in selection.iter() {
        copy_agent_instruction_files(&root, &source_dir, agent)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("godot-powertool-{name}-{unique}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn parses_agent_target_aliases() {
        assert_eq!(
            AgentTarget::from_str("claude-code"),
            Some(AgentTarget::ClaudeCode)
        );
        assert_eq!(AgentTarget::from_str("codex_cli"), Some(AgentTarget::Codex));
        assert_eq!(AgentTarget::from_str("cursor"), Some(AgentTarget::Cursor));
        assert_eq!(AgentTarget::from_str("none"), None);
        assert_eq!(AgentTarget::from_str("unknown"), None);
    }

    #[test]
    fn parses_agent_selection_bitmasks_and_names() {
        let cc_codex: AgentSelection = "3".parse().unwrap();
        assert!(cc_codex.contains(AgentTarget::ClaudeCode));
        assert!(cc_codex.contains(AgentTarget::Codex));
        assert!(!cc_codex.contains(AgentTarget::Cursor));

        let cc_cursor: AgentSelection = "claude,cursor".parse().unwrap();
        assert!(cc_cursor.contains(AgentTarget::ClaudeCode));
        assert!(!cc_cursor.contains(AgentTarget::Codex));
        assert!(cc_cursor.contains(AgentTarget::Cursor));

        let all: AgentSelection = "all".parse().unwrap();
        assert_eq!(all.iter().count(), 3);
        assert!("8".parse::<AgentSelection>().is_err());
    }

    #[test]
    fn maps_instruction_files_by_agent() {
        assert_eq!(AgentTarget::ClaudeCode.instruction_files(), &["CLAUDE.md"]);
        assert_eq!(AgentTarget::Codex.instruction_files(), &["AGENTS.md"]);
        assert_eq!(AgentTarget::Cursor.instruction_files(), &["AGENTS.md"]);
    }

    #[test]
    fn copies_instruction_files_without_overwriting_existing_work() {
        let root = temp_dir("agent-root");
        let templates = temp_dir("agent-templates");
        fs::write(templates.join("AGENTS.md"), "template").unwrap();
        fs::write(root.join("AGENTS.md"), "user-owned").unwrap();

        copy_agent_instruction_files(&root, &templates, AgentTarget::Codex).unwrap();

        assert_eq!(
            fs::read_to_string(root.join("AGENTS.md")).unwrap(),
            "user-owned"
        );
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(templates).unwrap();
    }

    #[test]
    fn cursor_mcp_merge_preserves_existing_servers() {
        let root = temp_dir("cursor-mcp");
        let cursor_dir = root.join(".cursor");
        fs::create_dir_all(&cursor_dir).unwrap();
        fs::write(
            cursor_dir.join("mcp.json"),
            r#"{"mcpServers":{"other":{"command":"other-bin"}}}"#,
        )
        .unwrap();

        install_mcp_for_cursor(&root, Path::new("/tmp/powertool-mcp")).unwrap();

        let config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(cursor_dir.join("mcp.json")).unwrap())
                .unwrap();
        assert_eq!(
            config["mcpServers"]["other"]["command"].as_str(),
            Some("other-bin")
        );
        assert_eq!(
            config["mcpServers"]["godot"]["type"].as_str(),
            Some("stdio")
        );
        assert_eq!(
            config["mcpServers"]["godot"]["command"].as_str(),
            Some("/tmp/powertool-mcp")
        );

        fs::remove_dir_all(root).unwrap();
    }
}
