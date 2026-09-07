//! Intent selection, ecosystem eligibility, graph expansion, and downward
//! specialization expansion.
//!
//! Selection runs in a fixed order: the initial selection (explicit keys, then
//! category/tag matches filtered by ecosystem); `follow`-edge expansion
//! outward from the selection; downward specialization expansion, which pulls
//! in every eligible record that `specializes` a selected one, transitively to
//! a fixpoint; and removal of general parents shadowed by a selected
//! specialization. Downward expansion runs only when
//! `graph.prefer_specializations` is true **and** the profile declares at
//! least one ecosystem — without a declared list it would pull every
//! language's version of every general record. Records it adds do not have
//! their own `follow` edges expanded: that keeps the pass bounded and
//! deterministic, and their `specializes` edge already points at a selected
//! parent.
//!
//! Rendering the selected records into guidance and enforcing the profile's
//! budget are cmf's job, downstream of [`select`].

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use anyhow::{Result, bail};

use crate::catalog::Intent;
use crate::profile::Profile;

/// The outcome of selecting intents for one profile: which records the
/// delivered artifact should carry, and how they were reached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selected {
    /// Intent keys retained for the delivered artifact, in catalog order.
    pub selected: BTreeSet<String>,
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
}

/// Select the intents one profile delivers from a scanned catalogue, in the
/// order the module header documents.
///
/// Fails when the profile names a missing or ineligible key, when a followed
/// edge points at a missing record, or when nothing is selected.
pub fn select(profile: &Profile, intents: &BTreeMap<String, Intent>) -> Result<Selected> {
    let InitialSelection {
        mut selected,
        excluded_by_ecosystem,
    } = initial_selection(profile, intents)?;
    let mut traversed = expand_graph(profile, intents, &mut selected)?;
    let mut specialized_downward = 0;
    if profile.graph.prefer_specializations {
        let pulled = specialize_downward(profile, intents, &mut selected);
        specialized_downward = pulled.len();
        traversed.extend(pulled);
        remove_shadowed_parents(intents, &mut selected);
    }
    if selected.is_empty() {
        bail!("profile selected no intents");
    }
    Ok(Selected {
        selected,
        traversed,
        excluded_by_ecosystem,
        specialized_downward,
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

/// Pull every eligible record that `specializes` a selected record into the
/// selection, transitively to a fixpoint, returning one traversal line per
/// pulled record in the form `<general-key> <--specializes-- <specialization-key>`.
///
/// This is the downward walk that lets a profile selecting general advice by
/// category and tag find the ecosystem's own version of it; the caller then
/// drops the shadowed general parent. It is a no-op when the profile declares
/// no ecosystems, because without that list it would pull every language's
/// specialization of every selected record. Records it adds are not expanded
/// along their own `follow` edges (see the module header).
pub fn specialize_downward(
    profile: &Profile,
    intents: &BTreeMap<String, Intent>,
    selected: &mut BTreeSet<String>,
) -> Vec<String> {
    if profile.select.ecosystems.is_empty() {
        return Vec::new();
    }
    let specializations_of = specialization_index(intents);
    let mut pulled = Vec::new();
    let mut queue: VecDeque<String> = selected.iter().cloned().collect();
    while let Some(general) = queue.pop_front() {
        let Some(specializations) = specializations_of.get(general.as_str()) else {
            continue;
        };
        for specialization in specializations {
            if !eligible(profile, specialization) {
                continue;
            }
            if selected.insert(specialization.key.clone()) {
                pulled.push(format!("{general} <--specializes-- {}", specialization.key));
                queue.push_back(specialization.key.clone());
            }
        }
    }
    pulled
}

/// Reverse index of `specializes` edges: general key to the records that
/// specialize it, in catalog order so downward expansion is deterministic.
fn specialization_index(intents: &BTreeMap<String, Intent>) -> BTreeMap<&str, Vec<&Intent>> {
    let mut index: BTreeMap<&str, Vec<&Intent>> = BTreeMap::new();
    for intent in intents.values() {
        for relation in &intent.record.relations {
            if relation.kind == "specializes" {
                index.entry(relation.target.as_str()).or_default().push(intent);
            }
        }
    }
    index
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{Evidence, IntentRecord, Relation};
    use crate::profile::{Content, Graph, Selection, Surface};

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

    /// The selected keys in catalog order, for comparison against literals.
    fn keys(selected: &Selected) -> Vec<&str> {
        selected.selected.iter().map(String::as_str).collect()
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
    fn explicit_key_is_selected() {
        let intents = BTreeMap::from([(
            "testing".to_string(),
            intent("testing", "quality", &["tests"], vec![]),
        )]);
        let profile = selecting(
            Selection {
                keys: vec!["testing".to_string()],
                ..Default::default()
            },
            Graph::default(),
        );
        let selected = select(&profile, &intents).unwrap();
        assert_eq!(keys(&selected), ["testing"]);
        assert!(selected.traversed.is_empty());
    }

    #[test]
    fn selecting_nothing_is_an_error() {
        let intents = multi_ecosystem_catalog();
        let profile = selecting(
            Selection {
                categories: strings(&["absent"]),
                tags: strings(&["testing"]),
                ..Default::default()
            },
            Graph::default(),
        );
        let error = select(&profile, &intents).unwrap_err().to_string();
        assert_eq!(error, "profile selected no intents");
    }

    #[test]
    fn category_tag_selection_admits_general_and_matching_records_only() {
        let intents = multi_ecosystem_catalog();
        let profile = selecting(category_tag_selection(&["python", "uv"]), keep_parents());

        let selected = select(&profile, &intents).unwrap();

        assert_eq!(
            keys(&selected),
            [
                "craftsperson/python/run-pytest",
                "craftsperson/python/uv/run-uv-pytest",
                "craftsperson/test-observable-behavior",
            ]
        );
        assert_eq!(selected.excluded_by_ecosystem, 1, "the Rust record is excluded");
    }

    #[test]
    fn declaring_python_alone_excludes_python_uv_records() {
        let intents = multi_ecosystem_catalog();
        let profile = selecting(category_tag_selection(&["python"]), keep_parents());

        let selected = select(&profile, &intents).unwrap();

        assert_eq!(
            keys(&selected),
            [
                "craftsperson/python/run-pytest",
                "craftsperson/test-observable-behavior"
            ]
        );
        assert_eq!(selected.excluded_by_ecosystem, 2, "Rust and Python/uv are excluded");
    }

    #[test]
    fn no_declared_ecosystems_applies_no_filter() {
        let intents = multi_ecosystem_catalog();
        let profile = selecting(category_tag_selection(&[]), keep_parents());

        let selected = select(&profile, &intents).unwrap();

        assert_eq!(selected.selected.len(), 4, "every record is admitted");
        assert_eq!(selected.excluded_by_ecosystem, 0);
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

        let error = select(&profile, &intents).unwrap_err().to_string();

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

        let selected = select(&profile, &intents).unwrap();

        assert_eq!(keys(&selected), ["craftsperson/python/uv/run-uv-pytest"]);
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

        let selected = select(&profile, &intents).unwrap();

        assert_eq!(
            selected.traversed,
            [
                "craftsperson/python/keep-tests-fast --related-to--> craftsperson/rust/run-cargo-test (skipped: ecosystem)",
                "craftsperson/python/keep-tests-fast --related-to--> craftsperson/python/run-pytest",
            ]
        );
        assert_eq!(
            keys(&selected),
            [
                "craftsperson/python/keep-tests-fast",
                "craftsperson/python/run-pytest"
            ]
        );
    }

    /// Select the general record only; every specialization is in another
    /// category so it can be reached solely by downward expansion.
    fn general_only_catalog() -> BTreeMap<String, Intent> {
        let mut intents = multi_ecosystem_catalog();
        for (key, intent) in &mut intents {
            if key != "craftsperson/test-observable-behavior" {
                intent.record.category = "specialized".to_string();
            }
        }
        intents
    }

    fn preferring_specializations() -> Graph {
        Graph {
            prefer_specializations: true,
            ..Graph::default()
        }
    }

    #[test]
    fn downward_expansion_pulls_the_declared_ecosystems_specialization_and_drops_the_parent() {
        let intents = general_only_catalog();
        let profile = selecting(category_tag_selection(&["python"]), preferring_specializations());

        let selected = select(&profile, &intents).unwrap();

        assert_eq!(keys(&selected), ["craftsperson/python/run-pytest"]);
        assert_eq!(
            selected.traversed,
            [
                "craftsperson/test-observable-behavior <--specializes-- craftsperson/python/run-pytest"
            ]
        );
        assert_eq!(selected.specialized_downward, 1);
    }

    #[test]
    fn downward_expansion_is_transitive_through_nested_ecosystems() {
        let mut intents = general_only_catalog();
        intents
            .get_mut("craftsperson/python/uv/run-uv-pytest")
            .unwrap()
            .record
            .relations = vec![Relation {
            kind: "specializes".to_string(),
            target: "craftsperson/python/run-pytest".to_string(),
        }];

        let both =
            selecting(category_tag_selection(&["python", "uv"]), preferring_specializations());
        let selected = select(&both, &intents).unwrap();
        assert_eq!(keys(&selected), ["craftsperson/python/uv/run-uv-pytest"]);
        assert_eq!(
            selected.traversed,
            [
                "craftsperson/test-observable-behavior <--specializes-- craftsperson/python/run-pytest",
                "craftsperson/python/run-pytest <--specializes-- craftsperson/python/uv/run-uv-pytest",
            ]
        );
        assert_eq!(selected.specialized_downward, 2);

        let python_only =
            selecting(category_tag_selection(&["python"]), preferring_specializations());
        let selected = select(&python_only, &intents).unwrap();
        assert_eq!(keys(&selected), ["craftsperson/python/run-pytest"]);
        assert_eq!(selected.specialized_downward, 1, "the uv record stays out");
    }

    #[test]
    fn no_declared_ecosystems_disables_downward_expansion() {
        let intents = general_only_catalog();
        let profile = selecting(category_tag_selection(&[]), preferring_specializations());

        let selected = select(&profile, &intents).unwrap();

        assert_eq!(keys(&selected), ["craftsperson/test-observable-behavior"]);
        assert!(selected.traversed.is_empty());
        assert_eq!(selected.specialized_downward, 0);
    }

    #[test]
    fn prefer_specializations_false_disables_downward_expansion() {
        let intents = general_only_catalog();
        let profile = selecting(category_tag_selection(&["python"]), keep_parents());

        let selected = select(&profile, &intents).unwrap();

        assert_eq!(keys(&selected), ["craftsperson/test-observable-behavior"]);
        assert!(selected.traversed.is_empty());
        assert_eq!(selected.specialized_downward, 0);
    }

    #[test]
    fn a_general_record_reached_by_a_followed_edge_also_gets_its_specialization() {
        let mut intents = general_only_catalog();
        intents.insert(
            "craftsperson/python/keep-tests-fast".to_string(),
            intent(
                "craftsperson/python/keep-tests-fast",
                "performance",
                &["speed"],
                vec![Relation {
                    kind: "related-to".to_string(),
                    target: "craftsperson/test-observable-behavior".to_string(),
                }],
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

        let selected = select(&profile, &intents).unwrap();

        assert_eq!(
            keys(&selected),
            [
                "craftsperson/python/keep-tests-fast",
                "craftsperson/python/run-pytest"
            ]
        );
        assert_eq!(
            selected.traversed,
            [
                "craftsperson/python/keep-tests-fast --related-to--> craftsperson/test-observable-behavior",
                "craftsperson/test-observable-behavior <--specializes-- craftsperson/python/run-pytest",
            ]
        );
        assert_eq!(selected.specialized_downward, 1);
    }

    #[test]
    fn an_already_selected_specialization_is_not_counted_as_pulled() {
        let intents = multi_ecosystem_catalog();
        let profile = selecting(category_tag_selection(&["python"]), preferring_specializations());

        let selected = select(&profile, &intents).unwrap();

        assert_eq!(keys(&selected), ["craftsperson/python/run-pytest"]);
        assert!(selected.traversed.is_empty());
        assert_eq!(selected.specialized_downward, 0);
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
