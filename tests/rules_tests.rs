use bfs::{metadata::Occurrence, rules::Rules};
/// Build a deterministic occurrence fixture for rules tests.
fn row(name: &str, id: char, hash: char) -> Occurrence {
    Occurrence {
        occurrence_id: id.to_string().repeat(64),
        sha256: hash.to_string().repeat(64),
        size: 3,
        file_type: "pdf".into(),
        original_name: name.into(),
        original_entry_name: name.into(),
        original_stem: name.into(),
        original_extension: "pdf".into(),
        source_archive: "a.zip".into(),
        source_path: name.into(),
        entry_index: 0,
        encrypted: true,
        ingest_date: "2026-10-29".into(),
        run_id: "run".into(),
    }
}
#[test]
/// Ensure rule ordering and duplicate policies produce expected plans.
fn first_match_and_duplicate_policies() {
    let records = vec![row("a", 'a', 'c'), row("b", 'b', 'c'), row("c", 'c', 'd')];
    for (policy, count) in [
        ("first_only", 2),
        ("all", 3),
        ("only_duplicates", 2),
        ("separate", 3),
    ] {
        let text = format!(
            "version = 1\n[organization]\nduplicates = '{policy}'\n[[rules]]\nname = 'first'\n[rules.match]\nfile_type = 'pdf'\nencrypted = true\n[rules.output]\ndirectory = 'encrypted'\nfilename = '{{original_name}}.sample'\n[[rules]]\nname = 'fallback'\n[rules.output]\ndirectory = 'fallback'\nfilename = '{{sha256}}.sample'"
        );
        let rules = Rules::parse(&text).unwrap();
        let plan = rules.plan(&records).unwrap();
        assert_eq!(plan.len(), count);
        assert!(plan.iter().all(|e| e.path.contains("encrypted/")));
        assert!(!plan.iter().any(|e| e.path.contains("fallback/")));
        if policy == "separate" {
            assert_eq!(
                plan.iter()
                    .filter(|e| e.path.starts_with("duplicates/"))
                    .count(),
                1
            );
        }
    }
}
#[test]
/// Ensure unsafe templates and unknown TOML fields fail validation.
fn unsafe_templates_and_unknown_fields_are_rejected() {
    for text in [
        "version = 2",
        "typo = true",
        "[organization]\ndefault_directory = '../escape'",
        "[organization]\ndefault_filename = '{bogus}.sample'",
        "[organization]\ndefault_filename = '{sha256}.exe'",
        "[organization]\ndefault_directory = '/absolute'",
    ] {
        assert!(Rules::parse(text).is_err(), "{text}");
    }
}
#[test]
/// Ensure hostile metadata is sanitized with deterministic collision handling.
fn hostile_names_and_case_collisions_are_safe_and_deterministic() {
    let rules = Rules::parse(
        "[organization]\nduplicates = 'all'\ndefault_filename = '{original_name}.sample'",
    )
    .unwrap();
    let records = vec![
        row("CON", 'a', 'a'),
        row("con", 'b', 'b'),
        row("../../escape\n", 'c', 'c'),
    ];
    let plan = rules.plan(&records).unwrap();
    assert_eq!(plan.len(), 3);
    assert!(
        plan.iter()
            .all(|e| !e.path.contains("../") && !e.path.contains('\n'))
    );
    assert_ne!(plan[0].path.to_lowercase(), plan[1].path.to_lowercase());
    let mut reversed = records.clone();
    reversed.reverse();
    assert_eq!(
        serde_json::to_value(&plan).unwrap(),
        serde_json::to_value(rules.plan(&reversed).unwrap()).unwrap()
    );
}
#[test]
/// Ensure reserved and file-directory collisions fail before filesystem work.
fn reserved_index_and_file_directory_collisions_fail_during_planning() {
    let rules = Rules::parse("[organization]\ndefault_directory = 'layout.jsonl'").unwrap();
    assert!(rules.plan(&[row("a", 'a', 'a')]).is_err());
    let rules = Rules::parse("[organization]\nduplicates = 'all'\ndefault_directory = '{source_archive}'\ndefault_filename = '{original_name}.sample'").unwrap();
    let mut a = row("a", 'a', 'a');
    a.source_archive = "root".into();
    let mut b = row("b", 'b', 'b');
    b.source_archive = "root".into();
    // Literal rule paths can create a directory beneath a prior sample path.
    let rules2 = Rules::parse("[organization]\nduplicates = 'all'\n[[rules]]\nname = 'a'\n[rules.match]\nmax_size = 3\n[rules.output]\ndirectory = 'root'\nfilename = 'a.sample'\n[[rules]]\nname = 'b'\n[rules.output]\ndirectory = 'root/a.sample'\nfilename = 'b.sample'").unwrap();
    b.size = 4;
    assert!(rules2.plan(&[a.clone(), b.clone()]).is_err());
    assert!(rules.plan(&[a, b]).is_ok());
}
#[test]
/// Ensure match ranges and Unicode normalization remain consistent.
fn match_fields_ranges_and_unicode_normalization_are_consistent() {
    let rules = Rules::parse("[organization]\nduplicates = 'all'\ndefault_directory = 'fallback'\n[[rules]]\nname = 'match'\n[rules.match]\noriginal_extension = 'PDF'\nduplicate = true\nmin_size = 3\nmax_size = 3\n[rules.output]\ndirectory = 'matched'\nfilename = '{original_name}.sample'").unwrap();
    let a = row("é", 'a', 'c');
    let b = row("e\u{301}", 'b', 'c');
    let c = row("other", 'c', 'd');
    let plan = rules.plan(&[a, b, c]).unwrap();
    assert_eq!(
        plan.iter()
            .filter(|e| e.path.starts_with("matched/"))
            .count(),
        2
    );
    assert_eq!(
        plan.iter()
            .filter(|e| e.path.starts_with("fallback/"))
            .count(),
        1
    );
    let names: std::collections::BTreeSet<_> = plan.iter().map(|e| &e.path).collect();
    assert_eq!(names.len(), 3);
    assert!(plan.iter().all(|e| !e.path.contains('\u{301}')));
    assert!(Rules::parse("[[rules]]\nname = 'bad'\n[rules.match]\nmin_size = 4\nmax_size = 3\n[rules.output]\ndirectory = 'x'\nfilename = 'x.sample'").is_err());
}
