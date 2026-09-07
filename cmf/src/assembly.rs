//! Intent selection, ecosystem eligibility, graph expansion, and
//! delivery-surface shaping.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::Write as _;

use anyhow::{Result, bail};

use crate::catalog::Intent;
use crate::profile::{Profile, Surface};

/// Materialized artifact plus build-time provenance.
#[derive(Debug)]
pub struct Assembly {
    /// Markdown agent or `SKILL.md` content.
    pub content: String,
    /// Intent keys retained in the delivered artifact.
    pub selected: Vec<String>,
    /// Graph edges followed during expansion. An edge to a record outside the
    /// profile's ecosystems is recorded with a `(skipped: ecosystem)` suffix.
    pub traversed: Vec<String>,
    /// Records that matched the profile's categories and tags but were
    /// excluded because they lie outside its declared ecosystems.
    pub excluded_by_ecosystem: usize,
    /// Approximate token count.
    pub estimated_tokens: usize,
}

/// Assemble one profile from a scanned intent catalogue.
pub fn assemble(profile: &Profile, intents: &BTreeMap<String, Intent>) -> Result<Assembly> {
    let InitialSelection {
        mut selected,
        excluded_by_ecosystem,
    } = initial_selection(profile, intents)?;
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
        excluded_by_ecosystem,
        estimated_tokens,
    })
}

/// The first ecosystem qualifier of `intent` that `profile` does not declare,
/// or `None` when the record is eligible for the profile.
///
/// A record is eligible when it has no qualifiers or every qualifier is among
/// `profile.select.ecosystems`; a profile declaring no ecosystems admits every
/// record. The returned qualifier names the reason a record was excluded.
pub fn excluding_qualifier<'i>(profile: &Profile, intent: &'i Intent) -> Option<&'i str> {
    let declared = &profile.select.ecosystems;
    if declared.is_empty() {
        return None;
    }
    intent
        .qualifiers()
        .into_iter()
        .find(|qualifier| !declared.iter().any(|ecosystem| ecosystem == qualifier))
}

fn eligible(profile: &Profile, intent: &Intent) -> bool {
    excluding_qualifier(profile, intent).is_none()
}

struct InitialSelection {
    selected: BTreeSet<String>,
    excluded_by_ecosystem: usize,
}

fn initial_selection(
    profile: &Profile,
    intents: &BTreeMap<String, Intent>,
) -> Result<InitialSelection> {
    let mut selected = BTreeSet::new();
    for key in &profile.select.keys {
        let Some(intent) = intents.get(key) else {
            bail!("profile references missing intent {key:?}");
        };
        if let Some(qualifier) = excluding_qualifier(profile, intent) {
            bail!(
                "profile selects intent {key:?} whose ecosystem qualifier {qualifier:?} is not among its declared ecosystems {:?}",
                profile.select.ecosystems
            );
        }
        selected.insert(key.clone());
    }
    let mut excluded_by_ecosystem = 0;
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
        if !(category && tag) {
            continue;
        }
        if eligible(profile, intent) {
            selected.insert(key.clone());
        } else {
            excluded_by_ecosystem += 1;
        }
    }
    Ok(InitialSelection {
        selected,
        excluded_by_ecosystem,
    })
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
            let Some(target) = intents.get(&relation.target) else {
                bail!("intent {key:?} references missing intent {:?}", relation.target);
            };
            if !eligible(profile, target) {
                traversed.push(format!(
                    "{key} --{}--> {} (skipped: ecosystem)",
                    relation.kind, relation.target
                ));
                continue;
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

fn write_evidence(out: &mut String, record: &crate::catalog::IntentRecord) {
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
    use crate::catalog::{Evidence, IntentRecord, Relation, STATIC_CHECK};
    use crate::profile::{Content, Graph, Selection};

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

    fn selecting(select: Selection, graph: Graph) -> Profile {
        Profile {
            id: "shipping".to_string(),
            name: None,
            version: "1.0.0".to_string(),
            description: "Use when shipping".to_string(),
            surface: Surface::Agent,
            budget_tokens: 5000,
            select,
            graph,
            content: Content::default(),
        }
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(ToString::to_string).collect()
    }

    /// One general testing record plus Rust, Python, and Python/uv
    /// specializations of it, all in category `quality` with tag `testing`.
    fn multi_ecosystem_catalog() -> BTreeMap<String, Intent> {
        let general = "craftsperson/test-observable-behavior";
        let specializes = || {
            vec![Relation {
                kind: "specializes".to_string(),
                target: general.to_string(),
            }]
        };
        [
            intent(general, "quality", &["testing"], vec![]),
            intent("craftsperson/rust/run-cargo-test", "quality", &["testing"], specializes()),
            intent("craftsperson/python/run-pytest", "quality", &["testing"], specializes()),
            intent("craftsperson/python/uv/run-uv-pytest", "quality", &["testing"], specializes()),
        ]
        .into_iter()
        .map(|intent| (intent.key.clone(), intent))
        .collect()
    }

    /// Keep general parents so the tests can observe them being admitted;
    /// `remove_shadowed_parents` is exercised separately and unchanged.
    fn keep_parents() -> Graph {
        Graph {
            prefer_specializations: false,
            ..Graph::default()
        }
    }

    fn category_tag_selection(ecosystems: &[&str]) -> Selection {
        Selection {
            categories: strings(&["quality"]),
            tags: strings(&["testing"]),
            ecosystems: strings(ecosystems),
            ..Default::default()
        }
    }

    #[test]
    fn category_tag_selection_admits_general_and_matching_records_only() {
        let intents = multi_ecosystem_catalog();
        let profile = selecting(category_tag_selection(&["python", "uv"]), keep_parents());

        let assembled = assemble(&profile, &intents).unwrap();

        assert_eq!(
            assembled.selected,
            [
                "craftsperson/python/run-pytest",
                "craftsperson/python/uv/run-uv-pytest",
                "craftsperson/test-observable-behavior",
            ]
        );
        assert_eq!(assembled.excluded_by_ecosystem, 1, "the Rust record is excluded");
    }

    #[test]
    fn declaring_python_alone_excludes_python_uv_records() {
        let intents = multi_ecosystem_catalog();
        let profile = selecting(category_tag_selection(&["python"]), keep_parents());

        let assembled = assemble(&profile, &intents).unwrap();

        assert_eq!(
            assembled.selected,
            [
                "craftsperson/python/run-pytest",
                "craftsperson/test-observable-behavior"
            ]
        );
        assert_eq!(assembled.excluded_by_ecosystem, 2, "Rust and Python/uv are excluded");
    }

    #[test]
    fn no_declared_ecosystems_applies_no_filter() {
        let intents = multi_ecosystem_catalog();
        let profile = selecting(category_tag_selection(&[]), keep_parents());

        let assembled = assemble(&profile, &intents).unwrap();

        assert_eq!(assembled.selected.len(), 4, "every record is admitted");
        assert_eq!(assembled.excluded_by_ecosystem, 0);
    }

    #[test]
    fn explicit_key_outside_declared_ecosystems_is_an_error_naming_the_qualifier() {
        let intents = multi_ecosystem_catalog();
        let profile = selecting(
            Selection {
                keys: strings(&["craftsperson/python/uv/run-uv-pytest"]),
                ecosystems: strings(&["python"]),
                ..Default::default()
            },
            Graph::default(),
        );

        let error = assemble(&profile, &intents).unwrap_err().to_string();

        assert_eq!(
            error,
            "profile selects intent \"craftsperson/python/uv/run-uv-pytest\" whose ecosystem qualifier \"uv\" is not among its declared ecosystems [\"python\"]"
        );
    }

    #[test]
    fn explicit_key_inside_declared_ecosystems_is_selected() {
        let intents = multi_ecosystem_catalog();
        let profile = selecting(
            Selection {
                keys: strings(&["craftsperson/python/uv/run-uv-pytest"]),
                ecosystems: strings(&["python", "uv"]),
                ..Default::default()
            },
            Graph::default(),
        );

        let assembled = assemble(&profile, &intents).unwrap();

        assert_eq!(assembled.selected, ["craftsperson/python/uv/run-uv-pytest"]);
    }

    #[test]
    fn graph_expansion_skips_ineligible_targets_and_records_the_skip() {
        let mut intents = multi_ecosystem_catalog();
        intents.insert(
            "craftsperson/python/keep-tests-fast".to_string(),
            intent(
                "craftsperson/python/keep-tests-fast",
                "performance",
                &["speed"],
                vec![
                    Relation {
                        kind: "related-to".to_string(),
                        target: "craftsperson/rust/run-cargo-test".to_string(),
                    },
                    Relation {
                        kind: "related-to".to_string(),
                        target: "craftsperson/python/run-pytest".to_string(),
                    },
                ],
            ),
        );
        let profile = selecting(
            Selection {
                keys: strings(&["craftsperson/python/keep-tests-fast"]),
                ecosystems: strings(&["python"]),
                ..Default::default()
            },
            Graph {
                follow: strings(&["related-to"]),
                max_related_depth: 1,
                prefer_specializations: true,
            },
        );

        let assembled = assemble(&profile, &intents).unwrap();

        assert_eq!(
            assembled.traversed,
            [
                "craftsperson/python/keep-tests-fast --related-to--> craftsperson/rust/run-cargo-test (skipped: ecosystem)",
                "craftsperson/python/keep-tests-fast --related-to--> craftsperson/python/run-pytest",
            ]
        );
        assert_eq!(
            assembled.selected,
            [
                "craftsperson/python/keep-tests-fast",
                "craftsperson/python/run-pytest"
            ]
        );
    }

    #[test]
    fn excluding_qualifier_names_the_first_undeclared_segment() {
        let intents = multi_ecosystem_catalog();
        let profile = selecting(category_tag_selection(&["rust"]), Graph::default());

        assert_eq!(
            excluding_qualifier(&profile, &intents["craftsperson/test-observable-behavior"]),
            None
        );
        assert_eq!(
            excluding_qualifier(&profile, &intents["craftsperson/rust/run-cargo-test"]),
            None
        );
        assert_eq!(
            excluding_qualifier(&profile, &intents["craftsperson/python/uv/run-uv-pytest"]),
            Some("python")
        );
    }
}
