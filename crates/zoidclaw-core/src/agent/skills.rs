//! Skills loader for agent capabilities.
//!
//! Skills are markdown files (`SKILL.md`) that teach the agent how to
//! perform specific tasks. Each skill lives in its own directory and
//! can have YAML frontmatter with metadata.

use std::path::{Path, PathBuf};

/// Loaded skill info.
#[derive(Debug, Clone)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
    pub source: String,
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
            lines.push(format!(
                "  <skill name=\"{}\" source=\"{}\">{}</skill>",
                skill.name, skill.source, skill.description
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

            let description = std::fs::read_to_string(&skill_file)
                .ok()
                .and_then(|c| extract_description(&c))
                .unwrap_or_else(|| format!("Skill: {}", name));

            out.push(SkillInfo {
                name,
                description,
                path: skill_file,
                source: source.to_owned(),
            });
        }
    }
}

/// Extract the `description` field from YAML frontmatter.
fn extract_description(content: &str) -> Option<String> {
    if !content.starts_with("---") {
        return None;
    }

    let end = content[3..].find("---")?;
    let frontmatter = &content[3..3 + end];

    for line in frontmatter.lines() {
        let line = line.trim();
        if let Some(desc) = line.strip_prefix("description:") {
            return Some(desc.trim().trim_matches('"').trim_matches('\'').to_string());
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
    fn test_strip_frontmatter() {
        let content = "---\ndescription: test\n---\n\nHello world";
        assert_eq!(strip_frontmatter(content), "Hello world");
    }

    #[test]
    fn test_extract_description() {
        let content = "---\ndescription: \"My cool skill\"\n---\n\nContent here";
        assert_eq!(extract_description(content), Some("My cool skill".into()));
    }

    #[test]
    fn test_no_frontmatter() {
        let content = "Just plain markdown";
        assert_eq!(strip_frontmatter(content), "Just plain markdown");
        assert_eq!(extract_description(content), None);
    }
}
