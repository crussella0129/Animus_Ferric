//! Skill Loader: reads `.ferric/skills/*/SKILL.md` to expose knowledge via MCP Prompts.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub content: String,
}

/// Parse YAML frontmatter from a SKILL.md file to extract `name` and `description`.
fn parse_frontmatter(content: &str) -> (String, String, String) {
    let mut name = String::new();
    let mut description = String::new();
    let mut body = content.to_string();

    if content.starts_with("---\n") {
        if let Some(end_idx) = content[4..].find("\n---\n") {
            let frontmatter = &content[4..4 + end_idx];
            body = content[4 + end_idx + 5..].trim_start().to_string();

            for line in frontmatter.lines() {
                if let Some(rest) = line.strip_prefix("name:") {
                    name = rest.trim().trim_matches('"').trim_matches('\'').to_string();
                } else if let Some(rest) = line.strip_prefix("description:") {
                    description = rest.trim().trim_matches('"').trim_matches('\'').to_string();
                }
            }
        }
    }
    (name, description, body)
}

/// Load all skills from `.ferric/skills/`.
pub fn load_skills(workspace: &Path) -> Vec<Skill> {
    let skills_dir = workspace.join(".ferric").join("skills");
    let mut skills = Vec::new();

    if let Ok(entries) = fs::read_dir(skills_dir) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                let skill_md = entry.path().join("SKILL.md");
                if let Ok(content) = fs::read_to_string(&skill_md) {
                    let (parsed_name, parsed_desc, body) = parse_frontmatter(&content);
                    
                    let dir_name = entry.file_name().to_string_lossy().to_string();
                    let name = if parsed_name.is_empty() { dir_name } else { parsed_name };
                    let description = if parsed_desc.is_empty() { "No description provided.".to_string() } else { parsed_desc };

                    skills.push(Skill {
                        name,
                        description,
                        content: body,
                    });
                }
            }
        }
    }
    skills
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn parse_valid_frontmatter() {
        let text = "---\nname: \"test_skill\"\ndescription: 'A test skill'\n---\nHere is the body.";
        let (name, desc, body) = parse_frontmatter(text);
        assert_eq!(name, "test_skill");
        assert_eq!(desc, "A test skill");
        assert_eq!(body, "Here is the body.");
    }

    #[test]
    fn parse_no_frontmatter() {
        let text = "Just a body.";
        let (name, desc, body) = parse_frontmatter(text);
        assert_eq!(name, "");
        assert_eq!(desc, "");
        assert_eq!(body, "Just a body.");
    }

    #[test]
    fn load_skills_from_dir() {
        let temp = TempDir::new().unwrap();
        let skills_dir = temp.path().join(".ferric").join("skills").join("deploy");
        fs::create_dir_all(&skills_dir).unwrap();
        fs::write(skills_dir.join("SKILL.md"), "---\nname: deploy\ndescription: How to deploy\n---\nRun deploy.sh").unwrap();

        let skills = load_skills(temp.path());
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "deploy");
        assert_eq!(skills[0].description, "How to deploy");
        assert_eq!(skills[0].content, "Run deploy.sh");
    }
}
