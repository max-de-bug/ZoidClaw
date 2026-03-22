//! Skills loader for agent capabilities.
//!
//! Skills are markdown files (`SKILL.md`) that teach the agent how to
//! perform specific tasks. Each skill lives in its own directory and
//! can have YAML frontmatter with metadata.
//!
//! Skills can declare an `intent-category` in frontmatter so they are
//! automatically loaded when the [`IntentRouter`] classifies a message
//! into a matching category.

use std::path::{Path, PathBuf};

use crate::tools::IntentCategory;

/// Loaded skill info.
#[derive(Debug, Clone)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
    pub source: String,
    /// Optional: the intent category this skill is associated with.
    /// When set, the skill is auto-loaded whenever the router
    /// classifies a message into this category.
    pub intent_category: Option<IntentCategory>,
    /// Whether the skill can be invoked directly by users (e.g. via
    /// a `/skill-name` slash command). Defaults to `false`.
    pub user_invocable: bool,
}

pub struct SkillsLoader {
    workspace_skills: PathBuf,
    builtin_skills: Option<PathBuf>,
}

impl SkillsLoader {
    pub fn new(workspace: &Path, builtin_skills: Option<PathBuf>) -> Self {
        Self {
            workspace_skills: workspace.join("skills"),
            builtin_skills,
        }
    }

    /// List all available skills from both workspace and builtin directories.
    pub fn list_skills(&self) -> Vec<SkillInfo> {
        let mut skills = Vec::new();

        // Workspace skills (custom, user-defined)
        self.scan_dir(&self.workspace_skills, "workspace", &mut skills);

        // Builtin skills (bundled with the binary)
        if let Some(ref builtin) = self.builtin_skills {
            self.scan_dir(builtin, "builtin", &mut skills);
        }

        skills
    }

    /// Return skill names that match the given intent category.
    ///
    /// This enables automatic skill activation: when the [`IntentRouter`]
    /// classifies a message, the agent loads all skills tagged with the
    /// matching category — zero user intervention required.
    pub fn skills_for_intent(&self, category: IntentCategory) -> Vec<String> {
        self.list_skills()
            .into_iter()
            .filter(|s| s.intent_category == Some(category))
            .map(|s| s.name)
            .collect()
    }

    /// Load a skill by name.
    pub fn load_skill(&self, name: &str) -> Option<String> {
        let skills = self.list_skills();
        let skill = skills.iter().find(|s| s.name == name)?;
        let content = std::fs::read_to_string(&skill.path).ok()?;
        Some(strip_frontmatter(&content))
    }

    /// Load multiple skills for inclusion in agent context.
    pub fn load_skills_for_context(&self, skill_names: &[String]) -> String {
        let mut parts = Vec::new();

        for name in skill_names {
            if let Some(content) = self.load_skill(name) {
                parts.push(format!("### Skill: {}\n{}", name, content));
            }
        }

        if parts.is_empty() {
            String::new()
        } else {
            format!("## Skills\n\n{}", parts.join("\n\n"))
        }
    }

    /// Build a summary of all available skills (name + description).
    pub fn build_summary(&self) -> String {
        let skills = self.list_skills();
        if skills.is_empty() {
            return String::new();
        }

        let mut lines = vec!["<skills>".to_owned()];
        for skill in &skills {
            let category_attr = skill
                .intent_category
                .map(|c| format!(" intent=\"{}\"", c.as_str()))
                .unwrap_or_default();
            lines.push(format!(
                "  <skill name=\"{}\" source=\"{}\"{}>{}</skill>",
                skill.name, skill.source, category_attr, skill.description
            ));
        }
        lines.push("</skills>".to_owned());
        lines.join("\n")
    }

    /// Scan a directory for skill subdirectories containing SKILL.md.
    fn scan_dir(&self, dir: &Path, source: &str, out: &mut Vec<SkillInfo>) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let skill_file = path.join("SKILL.md");
            if !skill_file.exists() {
                continue;
            }

            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();

            let raw_content = std::fs::read_to_string(&skill_file).ok();

            let description = raw_content
                .as_deref()
                .and_then(extract_description)
                .unwrap_or_else(|| format!("Skill: {}", name));

            let intent_category = raw_content
                .as_deref()
                .and_then(extract_intent_category);

            let user_invocable = raw_content
                .as_deref()
                .and_then(extract_user_invocable)
                .unwrap_or(false);

            out.push(SkillInfo {
                name,
                description,
                path: skill_file,
                source: source.to_owned(),
                intent_category,
                user_invocable,
            });
        }
    }
}

/// Extract the `description` field from YAML frontmatter.
fn extract_description(content: &str) -> Option<String> {
    extract_field(content, "description")
}

/// Extract the `intent-category` field from YAML frontmatter and parse
/// it into an [`IntentCategory`].
fn extract_intent_category(content: &str) -> Option<IntentCategory> {
    let raw = extract_field(content, "intent-category")?;
    match raw.to_lowercase().as_str() {
        "polymarket-read" | "polymarket_read" => Some(IntentCategory::PolymarketRead),
        "polymarket-trade" | "polymarket_trade" => Some(IntentCategory::PolymarketTrade),
        "crypto" | "crypto-tokens" | "crypto_tokens" => Some(IntentCategory::CryptoTokens),
        "prediction" => Some(IntentCategory::Prediction),
        "system" => Some(IntentCategory::System),
        "research" => Some(IntentCategory::Research),
        "general" => Some(IntentCategory::General),
        _ => None,
    }
}

/// Extract the `user-invocable` boolean field from YAML frontmatter.
fn extract_user_invocable(content: &str) -> Option<bool> {
    let raw = extract_field(content, "user-invocable")?;
    match raw.to_lowercase().as_str() {
        "true" | "yes" => Some(true),
        "false" | "no" => Some(false),
        _ => None,
    }
}

/// Generic field extractor for simple `key: value` YAML frontmatter.
///
/// Handles both single-line values and multi-line folded block scalars (`>`).
fn extract_field(content: &str, key: &str) -> Option<String> {
    if !content.starts_with("---") {
        return None;
    }

    let end = content[3..].find("---")?;
    let frontmatter = &content[3..3 + end];
    let prefix = format!("{}:", key);

    for line in frontmatter.lines() {
        let trimmed = line.trim();
        if let Some(val) = trimmed.strip_prefix(&prefix) {
            let val = val.trim().trim_matches('"').trim_matches('\'');
            // Handle folded block scalar (`>`) — join continuation lines
            if val == ">" || val.is_empty() {
                let rest: String = frontmatter
                    .lines()
                    .skip_while(|l| !l.trim().starts_with(&prefix))
                    .skip(1)
                    .take_while(|l| l.starts_with(' ') || l.starts_with('\t'))
                    .map(|l| l.trim())
                    .collect::<Vec<_>>()
                    .join(" ");
                return if rest.is_empty() { None } else { Some(rest) };
            }
            return Some(val.to_string());
        }
    }

    None
}

/// Remove YAML frontmatter from markdown content.
fn strip_frontmatter(content: &str) -> String {
    if !content.starts_with("---") {
        return content.to_string();
    }

    match content[3..].find("---") {
        Some(end) => content[3 + end + 3..].trim_start().to_string(),
        None => content.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_frontmatter_removes_yaml() {
        let content = "---\ndescription: test\n---\n\nHello world";
        assert_eq!(strip_frontmatter(content), "Hello world");
    }

    #[test]
    fn extract_description_simple() {
        let content = "---\ndescription: \"My cool skill\"\n---\n\nContent here";
        assert_eq!(extract_description(content), Some("My cool skill".into()));
    }

    #[test]
    fn extract_description_folded_block_scalar() {
        let content =
            "---\nname: test\ndescription: >\n  Advanced prediction market\n  analysis for Polymarket.\n---\n\nBody";
        let desc = extract_description(content).unwrap();
        assert!(desc.contains("Advanced prediction market"));
        assert!(desc.contains("analysis for Polymarket."));
    }

    #[test]
    fn extract_intent_category_parses_variants() {
        let content = "---\nintent-category: polymarket-read\n---\n";
        assert_eq!(
            extract_intent_category(content),
            Some(IntentCategory::PolymarketRead)
        );

        let content = "---\nintent-category: crypto\n---\n";
        assert_eq!(
            extract_intent_category(content),
            Some(IntentCategory::CryptoTokens)
        );
    }

    #[test]
    fn extract_user_invocable_parses_true() {
        let content = "---\nuser-invocable: true\n---\n";
        assert_eq!(extract_user_invocable(content), Some(true));
    }

    #[test]
    fn extract_user_invocable_defaults_none_if_missing() {
        let content = "---\nname: test\n---\n";
        assert_eq!(extract_user_invocable(content), None);
    }

    #[test]
    fn no_frontmatter_returns_none() {
        let content = "Just plain markdown";
        assert_eq!(strip_frontmatter(content), "Just plain markdown");
        assert_eq!(extract_description(content), None);
        assert_eq!(extract_intent_category(content), None);
    }

    #[test]
    fn extract_field_unquoted_value() {
        let content = "---\nname: my-skill\n---\n";
        assert_eq!(extract_field(content, "name"), Some("my-skill".into()));
    }
}
