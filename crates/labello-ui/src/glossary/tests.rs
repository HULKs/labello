use super::*;
use std::{collections::BTreeSet, path::Path};

#[test]
fn published_glossary_matches_the_ui_catalog() {
    let documentation = include_str!("../../../../docs/glossary.md");
    let table = documentation
        .split("<!-- glossary:start -->\n")
        .nth(1)
        .expect("glossary table start")
        .split("<!-- glossary:end -->")
        .next()
        .unwrap();
    let mut expected = String::from("| Name | Meaning |\n| --- | --- |\n");
    let mut names = BTreeSet::new();
    for entry in ENTRIES {
        assert!(
            names.insert(entry.name),
            "duplicate glossary term: {}",
            entry.name
        );
        assert!(!entry.definition.is_empty());
        expected.push_str(&format!("| {} | {} |\n", entry.name, entry.definition));
    }
    assert_eq!(
        table, expected,
        "update the published glossary with the catalog"
    );
}

#[test]
fn renderers_use_the_catalog_instead_of_copying_its_labels() {
    fn check(directory: &Path, violations: &mut Vec<String>) {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_str().unwrap();
            if matches!(
                name,
                "glossary" | "glossary.rs" | "ui_tests" | "inspector_presets.rs"
            ) || name.contains("test")
            {
                continue;
            }
            if path.is_dir() {
                check(&path, violations);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let source = std::fs::read_to_string(&path).unwrap();
                let production = source
                    .split("#[cfg(test)]\nmod ")
                    .next()
                    .unwrap()
                    .lines()
                    .filter(|line| !line.trim_start().starts_with("//"))
                    .collect::<Vec<_>>()
                    .join("\n");
                for entry in ENTRIES {
                    if production.contains(&format!("\"{}\"", entry.name)) {
                        violations.push(format!("{}: {}", path.display(), entry.name));
                    }
                }
                for retired in [
                    "Per Task",
                    "Task ID",
                    "model object",
                    "Model object",
                    "\"Backups\"",
                ] {
                    if production.contains(retired) {
                        violations.push(format!("{}: retired term {retired}", path.display()));
                    }
                }
            }
        }
    }
    let mut violations = Vec::new();
    check(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut violations,
    );
    assert!(
        violations.is_empty(),
        "use glossary terms:\n{}",
        violations.join("\n")
    );
}

#[test]
fn every_configurable_action_has_a_glossary_name() {
    for action in labello_domain::UserAction::ACTIVE {
        let label = shortcuts::action_label(&action);
        assert!(
            ENTRIES.iter().any(|entry| entry.name == label),
            "{action:?}: {label}"
        );
    }
    let primary = shortcuts::action_button_names(labello_domain::UserAction::NextImage);
    for name in [
        SUBMIT_NEXT,
        CONFIRM_NEXT,
        APPROVE,
        SUBMIT_CORRECTION,
        SAVE_NEXT,
    ] {
        assert!(primary.contains(name));
    }
    assert_ne!(SAVE, SUBMIT);
    assert_ne!(ANNOTATION, OBJECT);
    assert_ne!(PRELABEL, ANNOTATION);
}
