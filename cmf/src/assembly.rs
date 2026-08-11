//! Intent selection, graph expansion, and delivery-surface shaping.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::Write as _;

use anyhow::{Result, bail};

use crate::catalog::Intent;
use crate::profile::{Profile, Surface};

/// Materialized artifact plus build-time provenance.
pub struct Assembly {
    /// Markdown agent or `SKILL.md` content.
    pub content: String,
    /// Intent keys retained in the delivered artifact.
    pub selected: Vec<String>,
    /// Graph edges followed during expansion.
    pub traversed: Vec<String>,
    /// Approximate token count.
    pub estimated_tokens: usize,
}

/// Assemble one profile from a scanned intent catalogue.
pub fn assemble(profile: &Profile, intents: &BTreeMap<String, Intent>) -> Result<Assembly> {
    let mut selected = initial_selection(profile, intents)?;
    let traversed = expand_graph(profile, intents, &mut selected)?;
    if profile.graph.prefer_specializations {
        remove_shadowed_parents(intents, &mut selected);
    }
    if selected.is_empty() {
        bail!("profile selected no intents");
    }

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
        estimated_tokens,
    })
}

fn initial_selection(
    profile: &Profile,
    intents: &BTreeMap<String, Intent>,
) -> Result<BTreeSet<String>> {
    let mut selected = BTreeSet::new();
    for key in &profile.select.keys {
        if !intents.contains_key(key) {
            bail!("profile references missing intent {key:?}");
        }
        selected.insert(key.clone());
    }
    for (key, intent) in intents {
        let category = profile
            .select
            .categories
            .iter()
            .any(|candidate| candidate == &intent.record.category);
        let tag = profile
            .select
            .tags
            .iter()
            .any(|candidate| intent.record.tags.contains(candidate));
        if category && tag {
            selected.insert(key.clone());
        }
    }
    Ok(selected)
}

fn expand_graph(
    profile: &Profile,
    intents: &BTreeMap<String, Intent>,
    selected: &mut BTreeSet<String>,
) -> Result<Vec<String>> {
    let mut traversed = Vec::new();
    let mut queue: VecDeque<_> = selected.iter().cloned().map(|key| (key, 0_usize)).collect();
    while let Some((key, depth)) = queue.pop_front() {
        if depth >= profile.graph.max_related_depth {
            continue;
        }
        let intent = &intents[&key];
        for relation in &intent.record.relations {
            if !profile.graph.follow.contains(&relation.kind) {
                continue;
            }
            if !intents.contains_key(&relation.target) {
                bail!("intent {key:?} references missing intent {:?}", relation.target);
            }
            traversed.push(format!("{key} --{}--> {}", relation.kind, relation.target));
            if selected.insert(relation.target.clone()) {
                queue.push_back((relation.target.clone(), depth + 1));
            }
        }
    }
    Ok(traversed)
}

fn remove_shadowed_parents(intents: &BTreeMap<String, Intent>, selected: &mut BTreeSet<String>) {
    let parents: Vec<_> = selected
        .iter()
        .filter_map(|key| intents.get(key))
        .flat_map(|intent| {
            intent
                .record
                .relations
                .iter()
                .filter(|relation| relation.kind == "specializes")
                .map(|relation| relation.target.clone())
        })
        .filter(|parent| selected.contains(parent))
        .collect();
    for parent in parents {
        selected.remove(&parent);
    }
}

fn render(
    profile: &Profile,
    intents: &BTreeMap<String, Intent>,
    selected: &BTreeSet<String>,
) -> String {
    let name = profile.artifact_name();
    let mut out = format!(
        "---\nname: {name}\ndescription: {}\n---\n\n# {}\n\n",
        yaml_scalar(&profile.description),
        title_case(name)
    );
    if profile.surface == Surface::Skill {
        out.push_str("Use this guidance when the task matches the description above. Confirm the relevant repository evidence before applying specialized instructions.\n\n");
    }
    if profile.content.include.iter().any(|section| section == "guidance") {
        out.push_str("## Guidance\n\n");
        for key in selected {
            let intent = &intents[key];
            let _ = write!(out, "### {}\n\n{}\n\n", intent.record.title, intent.record.strategy);
        }
    }
    if profile.content.include.iter().any(|section| section == "rationale") {
        out.push_str("## Decision rationale\n\n");
        for key in selected {
            let record = &intents[key].record;
            let _ = writeln!(
                out,
                "- **{}:** {} This guards against {} Trade-off: {}",
                record.title, record.expectation, record.threat, record.tradeoff
            );
        }
        out.push('\n');
    }
    if profile.content.include.iter().any(|section| section == "evidence") {
        out.push_str("## Completion evidence\n\n");
        for key in selected {
            let record = &intents[key].record;
            for evidence in &record.evidence {
                let required = if evidence.required {
                    "Required"
                } else {
                    "Optional"
                };
                let _ = writeln!(
                    out,
                    "- **{} ({required}, {}):** {}",
                    record.title, evidence.kind, evidence.description
                );
            }
        }
    }
    out
}

fn yaml_scalar(value: &str) -> String {
    format!("{value:?}")
}
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{Evidence, IntentRecord, Relation};
    use crate::profile::{Content, Graph, Selection};

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
        assert!(assembled.content.contains("Apply testing."));
        assert_eq!(assembled.selected, vec!["testing"]);
    }
}
