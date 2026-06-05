//! XML skill context block builder for system prompt injection.
//!
//! Formats active skills into a structured `<skill_instructions>` XML block
//! that is injected between the base system prompt and tool descriptions (D-02).
//!
//! # Design (D-02)
//!
//! - Pure function: takes `&[LoadedSkill]`, returns `String`. No side effects.
//! - Skills are sorted by priority descending (highest first).
//! - Each skill is wrapped in a `<skill>` tag with `name`, `version`, and
//!   `priority` attributes.
//! - Body content is embedded directly between tags as raw Markdown.
//! - The output is deterministic — same input always produces same output.

use super::registry::LoadedSkill;

/// Build the `<skill_instructions>` XML block from active skills.
///
/// Skills are sorted by priority descending before formatting. The output is
/// meant to be placed between the base system prompt and tool descriptions in
/// the augmented system prompt (D-02).
///
/// # Arguments
///
/// * `active_skills` — the skills to include in the block, typically from
///   `SkillRegistry::get_active()`.
///
/// # Returns
///
/// An XML-formatted skill instruction block. Returns
/// `"<skill_instructions>\n</skill_instructions>"` when there are no active
/// skills (empty input).
pub fn build_skill_context_block(active_skills: &[LoadedSkill]) -> String {
    if active_skills.is_empty() {
        return "<skill_instructions>\n</skill_instructions>".to_string();
    }

    // Sort by priority descending (highest first) per D-02.
    let mut sorted = active_skills.to_vec();
    sorted.sort_by(|a, b| b.priority.cmp(&a.priority));

    let mut block = String::from("<skill_instructions>\n");

    for skill in &sorted {
        let version = skill.manifest.version.as_deref().unwrap_or("unknown");
        block.push_str(&format!(
            "<skill name=\"{name}\" version=\"{version}\" priority=\"{priority}\">\n{body}\n</skill>\n",
            name = skill.manifest.name,
            version = version,
            priority = skill.priority,
            body = skill.body,
        ));
    }

    block.push_str("</skill_instructions>");
    block
}

#[cfg(test)]
mod tests {
    use super::*;
    use jadepaw_core::SkillId;

    fn make_skill(name: &str, body: &str, priority: u8, version: Option<&str>) -> LoadedSkill {
        LoadedSkill {
            skill_id: SkillId::new(),
            manifest: jadepaw_core::SkillManifest {
                name: name.to_string(),
                description: format!("Skill {}", name),
                tools: vec![],
                constraints: None,
                version: version.map(|v| v.to_string()),
                author: None,
                metadata: None,
                source_path: std::path::PathBuf::from("/test/SKILL.md"),
            },
            body: body.to_string(),
            priority,
            loaded_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn empty_skills_produces_empty_block() {
        let block = build_skill_context_block(&[]);
        assert_eq!(block, "<skill_instructions>\n</skill_instructions>");
    }

    #[test]
    fn single_skill() {
        let skills = [make_skill("code-reviewer", "Review all code changes.", 0, Some("0.1.0"))];
        let block = build_skill_context_block(&skills);
        assert!(block.contains("<skill name=\"code-reviewer\" version=\"0.1.0\" priority=\"0\">"));
        assert!(block.contains("Review all code changes."));
        assert!(block.contains("</skill_instructions>"));
    }

    #[test]
    fn version_defaults_to_unknown() {
        let skills = [make_skill("my-skill", "Body text", 0, None)];
        let block = build_skill_context_block(&skills);
        assert!(block.contains("version=\"unknown\""));
    }

    #[test]
    fn multiple_skills_sorted_by_priority() {
        let low = make_skill("low", "Low pri", 1, None);
        let high = make_skill("high", "High pri", 10, None);
        let mid = make_skill("mid", "Mid pri", 5, None);

        let skills = [low, high, mid];
        let block = build_skill_context_block(&skills);

        // Find the order of skill names in the output
        let high_pos = block.find("high").unwrap();
        let mid_pos = block.find("mid").unwrap();
        let low_pos = block.find("low").unwrap();

        assert!(
            high_pos < mid_pos,
            "high priority should appear before mid"
        );
        assert!(
            mid_pos < low_pos,
            "mid priority should appear before low"
        );
    }

    #[test]
    fn markdown_body_preserved_verbatim() {
        let body = "# Instructions\n\n- Step 1\n- Step 2\n\n```rust\nfn main() {}\n```";
        let skills = [make_skill("test", body, 0, None)];
        let block = build_skill_context_block(&skills);
        assert!(block.contains("# Instructions"));
        assert!(block.contains("- Step 1"));
        assert!(block.contains("```rust"));
        assert!(block.contains("fn main() {}"));
    }

    #[test]
    fn deterministic_output() {
        let skills = [
            make_skill("skill-a", "A", 0, None),
            make_skill("skill-b", "B", 0, None),
        ];
        let block1 = build_skill_context_block(&skills);
        let block2 = build_skill_context_block(&skills);
        assert_eq!(block1, block2);
    }
}