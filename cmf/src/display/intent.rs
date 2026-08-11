//! Human-readable output for intent records.

use std::fmt;

use cmx::table::render_table;

use crate::intent::IntentList;

impl fmt::Display for IntentList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Intents ({}):", self.0.len())?;
        if self.0.is_empty() {
            return Ok(());
        }

        let rows = self
            .0
            .iter()
            .map(|intent| {
                vec![
                    intent.key.clone(),
                    intent.record.category.clone(),
                    intent.record.title.clone(),
                ]
            })
            .collect();
        write!(f, "{}", render_table(vec!["Key", "Category", "Title"], 2, rows))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::intent::{Intent, IntentEvidence, IntentList, IntentRecord, IntentSource};

    #[test]
    fn displays_intent_identity() {
        let list = IntentList(vec![Intent {
            key: "craftsperson/verify".to_string(),
            record: IntentRecord {
                id: "guidelines.intent.verify".to_string(),
                title: "Verify before completion".to_string(),
                category: "quality".to_string(),
                tags: vec!["verification".to_string()],
                status: "hypothesized".to_string(),
                confidence: 0.99,
                capability: "Trustworthy completion".to_string(),
                threat: "Unchecked work".to_string(),
                expectation: "Checks find defects".to_string(),
                strategy: "Run checks".to_string(),
                tradeoff: "Takes time".to_string(),
                relations: Vec::new(),
                evidence: vec![IntentEvidence {
                    kind: "gate".to_string(),
                    description: "Checks pass".to_string(),
                    required: true,
                }],
                scope: toml::Table::from_iter([(
                    "project".to_string(),
                    toml::Value::String("guidelines".to_string()),
                )]),
                sources: vec![IntentSource {
                    kind: "document".to_string(),
                    reference: "agents/test.md".to_string(),
                    summary: "Requires checks".to_string(),
                    confidence: 1.0,
                }],
            },
            path: PathBuf::from("/repo/intents/craftsperson/verify.toml"),
        }]);

        let output = list.to_string();
        assert!(output.contains("craftsperson/verify"));
        assert!(output.contains("quality"));
        assert!(output.contains("Verify before completion"));
    }
}
