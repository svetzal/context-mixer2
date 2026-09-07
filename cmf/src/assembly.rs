//! Delivery-surface shaping: render the intents `intent_atlas::selection`
//! selected into a Markdown agent or `SKILL.md`, and enforce the profile's
//! token budget.
//!
//! Assembly runs in a fixed order: selection (see
//! [`intent_atlas::selection::select`] for the initial selection, `follow`
//! expansion, downward specialization, and shadowed-parent removal), then
//! rendering, then the budget check. Everything before rendering lives in the
//! `intent-atlas` crate because cmv needs it too; rendering and the budget are
//! cmf's alone.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use anyhow::{Result, bail};
use intent_atlas::catalog::Intent;
use intent_atlas::profile::{Profile, Surface};
use intent_atlas::selection::{Selected, select};

/// Materialized artifact plus build-time provenance.
#[derive(Debug)]
pub struct Assembly {
    /// Markdown agent or `SKILL.md` content.
    pub content: String,
    /// Intent keys retained in the delivered artifact.
    pub selected: Vec<String>,
    /// Graph edges followed during expansion. An edge to a record outside the
    /// profile's ecosystems is recorded with a `(skipped: ecosystem)` suffix;
    /// a specialization pulled in by downward expansion is recorded as
    /// `<general-key> <--specializes-- <specialization-key>`.
    pub traversed: Vec<String>,
    /// Records that matched the profile's categories and tags but were
    /// excluded because they lie outside its declared ecosystems.
    pub excluded_by_ecosystem: usize,
    /// Eligible specializations of selected records pulled in by downward
    /// expansion; zero when either of its gates does not hold.
    pub specialized_downward: usize,
    /// Approximate token count.
    pub estimated_tokens: usize,
}

/// Assemble one profile from a scanned intent catalogue.
pub fn assemble(profile: &Profile, intents: &BTreeMap<String, Intent>) -> Result<Assembly> {
    let Selected {
        selected,
        traversed,
        excluded_by_ecosystem,
        specialized_downward,
    } = select(profile, intents)?;

    let content = render(profile, intents, &selected);
    let estimated_tokens = content.chars().count().div_ceil(4);
    if estimated_tokens > profile.budget_tokens {
        bail!(
            "assembled artifact needs approximately {estimated_tokens} tokens, exceeding budget {}",
            profile.budget_tokens
        );
    }
    Ok(Assembly {
        content,
        selected: selected.into_iter().collect(),
        traversed,
        excluded_by_ecosystem,
        specialized_downward,
        estimated_tokens,
    })
}

fn render(
    profile: &Profile,
    intents: &BTreeMap<String, Intent>,
    selected: &BTreeSet<String>,
) -> String {
    let name = profile.artifact_name();
    let mut out =
        format!("---\nname: {name}\ndescription: {}\n---\n\n", yaml_scalar(&profile.description));
    if profile.surface == Surface::Skill {
        out.push_str("Use this guidance when the task matches the description above. Confirm the relevant repository evidence before applying specialized instructions.\n\n");
    }
    let guidance = profile.content.include.iter().any(|section| section == "guidance");
    let rationale = profile.content.include.iter().any(|section| section == "rationale");
    let evidence = profile.content.include.iter().any(|section| section == "evidence");
    for key in selected {
        let record = &intents[key].record;
        out.push('-');
        if rationale {
            write_clause(&mut out, "Preserve", &record.capability);
            write_connector(&mut out, "Because", &record.threat);
            write_clause(&mut out, "Expect", &record.expectation);
        }
        if guidance {
            write_clause(&mut out, "Prefer", &record.strategy);
        }
        if evidence {
            write_evidence(&mut out, record);
        }
        if rationale {
            write_clause(&mut out, "Accept", &record.tradeoff);
        }
        out.push_str("\n\n");
    }
    out
}

fn write_clause(out: &mut String, label: &str, value: &str) {
    let _ = write!(out, " {label}: {}.", trim_terminal_punctuation(value));
}

fn write_connector(out: &mut String, connector: &str, value: &str) {
    let _ = write!(out, " {connector} {}.", trim_terminal_punctuation(value));
}

fn write_evidence(out: &mut String, record: &intent_atlas::catalog::IntentRecord) {
    let required: Vec<_> = record
        .evidence
        .iter()
        .filter(|item| item.required)
        .map(|item| trim_terminal_punctuation(&item.description))
        .collect();
    if !required.is_empty() {
        write_clause(out, "Require", &required.join("; "));
    }
    let optional: Vec<_> = record
        .evidence
        .iter()
        .filter(|item| !item.required)
        .map(|item| trim_terminal_punctuation(&item.description))
        .collect();
    if !optional.is_empty() {
        write_clause(out, "Observe when useful", &optional.join("; "));
    }
}

fn trim_terminal_punctuation(value: &str) -> &str {
    value.trim().trim_end_matches(['.', '!', '?'])
}

fn yaml_scalar(value: &str) -> String {
    format!("{value:?}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use intent_atlas::catalog::{Evidence, IntentRecord, Relation, STATIC_CHECK};
    use intent_atlas::profile::{Content, Graph, Selection};

    fn title_case(value: &str) -> String {
        value
            .split(['-', '_'])
            .map(|word| {
                let mut chars = word.chars();
                chars.next().map_or_else(String::new, |first| {
                    first.to_uppercase().collect::<String>() + chars.as_str()
                })
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn intent(key: &str, category: &str, tags: &[&str], relations: Vec<Relation>) -> Intent {
        Intent {
            key: key.to_string(),
            path: key.into(),
            record: IntentRecord {
                id: key.to_string(),
                title: title_case(key),
                category: category.to_string(),
                tags: tags.iter().map(ToString::to_string).collect(),
                status: "hypothesized".to_string(),
                capability: "Good result.".to_string(),
                threat: "Bad result.".to_string(),
                expectation: "This helps.".to_string(),
                strategy: format!("Apply {key}."),
                tradeoff: "Some effort.".to_string(),
                relations,
                evidence: vec![Evidence {
                    kind: "gate".to_string(),
                    description: "The gate passes.".to_string(),
                    required: true,
                    language: None,
                    run: None,
                }],
            },
        }
    }

    #[test]
    fn explicit_profile_assembles_skill() {
        let intents = BTreeMap::from([(
            "testing".to_string(),
            intent("testing", "quality", &["tests"], vec![]),
        )]);
        let profile = Profile {
            id: "test-work".to_string(),
            name: None,
            version: "1.0.0".to_string(),
            description: "Use for test work".to_string(),
            surface: Surface::Skill,
            budget_tokens: 500,
            select: Selection {
                keys: vec!["testing".to_string()],
                ..Default::default()
            },
            graph: Graph::default(),
            content: Content::default(),
        };
        let assembled = assemble(&profile, &intents).unwrap();
        assert!(assembled.content.contains("- Prefer: Apply testing. Require: The gate passes."));
        assert!(!assembled.content.contains("## Guidance"));
        assert_eq!(assembled.selected, vec!["testing"]);
    }

    #[test]
    fn renders_each_intent_as_one_ordered_block() {
        let intents = BTreeMap::from([(
            "testing".to_string(),
            intent("testing", "quality", &["tests"], vec![]),
        )]);
        let profile = Profile {
            id: "test-work".to_string(),
            name: None,
            version: "1.0.0".to_string(),
            description: "Use for test work".to_string(),
            surface: Surface::Agent,
            budget_tokens: 500,
            select: Selection {
                keys: vec!["testing".to_string()],
                ..Default::default()
            },
            graph: Graph::default(),
            content: Content {
                include: vec![
                    "guidance".to_string(),
                    "rationale".to_string(),
                    "evidence".to_string(),
                ],
            },
        };

        let content = assemble(&profile, &intents).unwrap().content;
        let block = "- Preserve: Good result. Because Bad result. Expect: This helps. Prefer: Apply testing. Require: The gate passes. Accept: Some effort.";
        assert!(content.contains(block));
        assert!(!content.contains("Testing"));
        assert!(!content.contains("##"));
    }

    #[test]
    fn validator_evidence_renders_only_its_description() {
        let mut testing = intent("testing", "quality", &["tests"], vec![]);
        testing.record.evidence.push(Evidence {
            kind: STATIC_CHECK.to_string(),
            description: "No effectful call leaves a gateway module.".to_string(),
            required: true,
            language: Some("rust".to_string()),
            run: Some("checks/rust/gateways.py".to_string()),
        });
        let intents = BTreeMap::from([("testing".to_string(), testing)]);
        let profile = Profile {
            id: "test-work".to_string(),
            name: None,
            version: "1.0.0".to_string(),
            description: "Use for test work".to_string(),
            surface: Surface::Agent,
            budget_tokens: 500,
            select: Selection {
                keys: vec!["testing".to_string()],
                ..Default::default()
            },
            graph: Graph::default(),
            content: Content::default(),
        };

        let content = assemble(&profile, &intents).unwrap().content;
        assert!(
            content
                .contains("Require: The gate passes; No effectful call leaves a gateway module.")
        );
        assert!(!content.contains("checks/rust/gateways.py"));
        assert!(!content.contains("rust"));
        assert!(!content.contains(STATIC_CHECK));
    }

    #[test]
    fn exceeding_the_budget_is_an_error_naming_both_counts() {
        let intents = BTreeMap::from([(
            "testing".to_string(),
            intent("testing", "quality", &["tests"], vec![]),
        )]);
        let profile = Profile {
            id: "test-work".to_string(),
            name: None,
            version: "1.0.0".to_string(),
            description: "Use for test work".to_string(),
            surface: Surface::Agent,
            budget_tokens: 1,
            select: Selection {
                keys: vec!["testing".to_string()],
                ..Default::default()
            },
            graph: Graph::default(),
            content: Content::default(),
        };

        let error = assemble(&profile, &intents).unwrap_err().to_string();
        assert!(error.starts_with("assembled artifact needs approximately "), "{error}");
        assert!(error.ends_with(" tokens, exceeding budget 1"), "{error}");
    }
}
