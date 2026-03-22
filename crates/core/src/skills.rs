use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::AgentError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillDefinition {
    pub name: String,
    pub instructions: String,
}

#[derive(Debug, Clone, Default)]
pub struct SkillRegistry {
    definitions: HashMap<String, SkillDefinition>,
}

impl SkillRegistry {
    pub fn register(&mut self, name: impl Into<String>, instructions: impl Into<String>) -> bool {
        let name = name.into();
        let instructions = instructions.into().trim().to_string();
        if instructions.is_empty() {
            return false;
        }
        self.definitions
            .insert(name.clone(), SkillDefinition { name, instructions });
        true
    }

    pub fn get(&self, name: &str) -> Option<&SkillDefinition> {
        self.definitions.get(name)
    }

    pub fn load_from_dir(&mut self, dir: impl AsRef<Path>) -> Result<usize, AgentError> {
        let mut loaded = 0;
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let content = fs::read_to_string(&path)?;
            if self.register(stem.to_string(), content) {
                loaded += 1;
            }
        }
        Ok(loaded)
    }

    pub fn resolve_from_input(&self, input: &str) -> Vec<SkillDefinition> {
        let mut active = Vec::new();
        for token in input.split_whitespace() {
            let Some(name) = token.strip_prefix('@') else {
                continue;
            };
            if let Some(skill) = self.get(name) {
                active.push(skill.clone());
            }
        }
        active
    }
}
