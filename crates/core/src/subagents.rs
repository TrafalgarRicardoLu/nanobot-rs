use std::collections::HashMap;
use std::sync::Arc;

type SubagentHandler = Arc<dyn Fn(&str) -> Result<String, String> + Send + Sync>;

#[derive(Clone, Default)]
pub struct SubagentManager {
    handlers: HashMap<String, SubagentHandler>,
}

impl std::fmt::Debug for SubagentManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut names = self.handlers.keys().cloned().collect::<Vec<_>>();
        names.sort();
        f.debug_struct("SubagentManager")
            .field("handlers", &names)
            .finish()
    }
}

impl SubagentManager {
    pub fn register_static(&mut self, name: impl Into<String>, response_prefix: impl Into<String>) {
        let response_prefix = response_prefix.into();
        self.handlers.insert(
            name.into(),
            Arc::new(move |task: &str| Ok(format!("{response_prefix}: {task}"))),
        );
    }

    pub fn run(&self, name: &str, task: &str) -> Result<String, String> {
        let handler = self
            .handlers
            .get(name)
            .ok_or_else(|| format!("unknown subagent: {name}"))?;
        handler(task)
    }
}
