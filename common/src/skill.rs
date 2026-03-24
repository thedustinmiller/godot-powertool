use std::path::{Path, PathBuf};

/// Target location for installing agent skill files.
#[derive(Debug, Clone)]
pub enum SkillTarget {
    /// Claude Code: `.claude/skills/`
    ClaudeCode,
    /// OpenAI Codex CLI: `.agents/skills/`
    Codex,
    /// Generic: `skills/`
    Generic,
    /// Custom path provided by user
    Custom(PathBuf),
}

impl SkillTarget {
    /// Resolve the skill installation directory relative to a project root.
    pub fn skill_dir(&self, project_root: &Path) -> PathBuf {
        match self {
            Self::ClaudeCode => project_root.join(".claude").join("skills"),
            Self::Codex => project_root.join(".agents").join("skills"),
            Self::Generic => project_root.join("skills"),
            Self::Custom(path) => path.clone(),
        }
    }

    /// Parse a skill target from a string flag value.
    pub fn from_str_or_path(s: &str) -> Self {
        match s {
            "claude" => Self::ClaudeCode,
            "codex" => Self::Codex,
            "generic" => Self::Generic,
            path => Self::Custom(PathBuf::from(path)),
        }
    }
}

impl Default for SkillTarget {
    fn default() -> Self {
        Self::ClaudeCode
    }
}
