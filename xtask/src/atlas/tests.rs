//! Native presentation and consumer boundaries; shared lexical laws live in Atlas.
use super::*;
use atlas_core::{discovery::Snapshot, population::FileSelection, revision::Revision};
use std::{fs, path::Path, process::Command};
#[test]
fn invocation_validation_rejects_silent_flag_and_shape_mismatches() {
    assert!(
        validate_invocation(
            "comments",
            None,
            AtlasFlags {
                audit: true,
                ..AtlasFlags::default()
            }
        )
        .is_ok()
    );
    assert!(
        validate_invocation(
            "module",
            Some("src/lib.rs"),
            AtlasFlags {
                items: true,
                graph: true,
                ..AtlasFlags::default()
            }
        )
        .is_ok()
    );

    assert!(
        validate_invocation(
            "census",
            None,
            AtlasFlags {
                audit: true,
                ..AtlasFlags::default()
            }
        )
        .expect_err("--audit is irrelevant")
        .contains("does not accept --audit")
    );
    assert!(
        validate_invocation(
            "concentration",
            None,
            AtlasFlags {
                items: true,
                ..AtlasFlags::default()
            }
        )
        .expect_err("--items is irrelevant")
        .contains("does not accept --items")
    );
    assert!(
        validate_invocation(
            "comments",
            None,
            AtlasFlags {
                audit: true,
                graph: true,
                ..AtlasFlags::default()
            }
        )
        .expect_err("--graph is irrelevant")
        .contains("does not accept --graph")
    );
    assert!(
        validate_invocation("dump", None, AtlasFlags::default())
            .expect_err("dump requires JSON")
            .contains("requires --json")
    );
    assert!(
        validate_invocation("unknown", None, AtlasFlags::default())
            .expect_err("unknown command")
            .contains("unknown atlas verb")
    );
}

struct Fixture {
    directory: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let f = Self {
            directory: tempfile::tempdir().expect("fixture"),
        };
        f.write(
            "Cargo.toml",
            "[workspace]\nmembers=[\"a\"]\nresolver=\"3\"\n",
        );
        f.write(
            "a/Cargo.toml",
            "[package]\nname=\"sample\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
        );
        f.write("a/src/lib.rs","//! Fixture crate root.\npub mod admit;\npub mod grade;\npub use crate::admit::Tile;\n");
        f.write("a/src/admit.rs","//! Admission for fixture tiles — realization kernel §5.\nuse crate::grade::Cost;\nuse std::fmt;\n/// An admitted fixture tile.\npub struct Tile {\n    cost: Cost,\n}\n/// Refusal for the tile family.\npub(crate) enum Refusal {\n    CrossLane,\n}\npub fn admit(cost: Cost) -> Tile {\n    Tile { cost }\n}\nfn private_helper() {}\n#[cfg(test)]\nmod tests {\n    use super::*;\n}\n");
        f.write("a/src/grade.rs","pub struct Cost;\npub use self::Cost as PublicCost;\nmacro_rules! grade_table {\n    () => {};\n}\n");
        f
    }
    fn root(&self) -> &Path {
        self.directory.path()
    }
    fn write(&self, path: &str, text: &str) {
        let p = self.root().join(path);
        fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
        fs::write(p, text).expect("write");
    }
    fn snapshot(&self) -> Snapshot {
        let metadata = cargo_metadata::MetadataCommand::new()
            .current_dir(self.root())
            .no_deps()
            .exec()
            .expect("metadata");
        input::capture_metadata(metadata).expect("snapshot")
    }
    fn git(&self, args: &[&str]) -> String {
        let mut command = Command::new("git");
        for (key, _) in
            std::env::vars_os().filter(|(key, _)| key.to_string_lossy().starts_with("GIT_"))
        {
            command.env_remove(key);
        }
        let output = command
            .current_dir(self.root())
            .args(args)
            .output()
            .expect("fixture Git");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("UTF8")
    }
    fn commit(&self) {
        self.git(&["init", "--quiet"]);
        self.git(&["add", "."]);
        self.git(&[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "fixture",
        ]);
    }
}
const PUBISH: Filter = Filter {
    pub_only: false,
    include_private: false,
};
const ALL: Filter = Filter {
    pub_only: false,
    include_private: true,
};
fn value(text: impl AsRef<str>) -> serde_json::Value {
    serde_json::from_str(text.as_ref()).expect("report JSON")
}

#[test]
fn census_ladder_and_visibility_preserve_native_units() {
    let f = Fixture::new();
    let s = f.snapshot();
    let n = Navigation::new(&s, PUBISH.selection()).expect("navigation");
    let workspace = value(census(&n, None, PUBISH, true).expect("census"));
    assert_eq!(workspace["rows"][0]["types"], 3);
    assert_eq!(workspace["rows"][0]["fns"], 1);
    assert_eq!(workspace["rows"][0]["aliases"], 2);
    assert_eq!(workspace["rows"][0]["macros"], 1);
    for key in ["sample", "a/src"] {
        assert_eq!(
            value(census(&n, Some(key), PUBISH, true).expect("rung"))["rows"]
                .as_array()
                .expect("rows")
                .len(),
            3
        );
    }
    assert_eq!(
        value(census(&n, Some("a/src/admit.rs"), PUBISH, true).expect("module"))["verb"],
        "module"
    );
    let public = Filter {
        pub_only: true,
        include_private: false,
    };
    let n = Navigation::new(&s, public.selection()).expect("navigation");
    assert_eq!(
        value(census(&n, None, public, true).expect("pub"))["rows"][0]["types"],
        2
    );
}
#[test]
fn name_module_dump_and_comment_policy_share_captured_facts() {
    let f = Fixture::new();
    let s = f.snapshot();
    let n = Navigation::new(&s, PUBISH.selection()).expect("navigation");
    let name = value(name_card(&n, "Tile", PUBISH, true).expect("name"));
    assert_eq!(name["matches"].as_array().expect("matches").len(), 1);
    assert_eq!(name["reexports"].as_array().expect("exports").len(), 1);
    assert_eq!(
        name["edges"]["produced_by"]
            .as_array()
            .expect("edges")
            .len(),
        1
    );
    let module =
        value(module_card(&n, "a/src/admit.rs", PUBISH, true, true, true).expect("module"));
    assert_eq!(
        module["header"],
        "Admission for fixture tiles — realization kernel §5."
    );
    assert_eq!(module["census"]["types"], 2);
    assert!(
        module["graph"]
            .as_array()
            .expect("edges")
            .iter()
            .any(|e| e["source"] == "admit" && e["class"] == "consumes" && e["target"] == "Cost")
    );
    let n = Navigation::new(&s, ALL.selection()).expect("navigation");
    let index = value(dump(&n, &s, ALL).expect("dump"));
    assert!(
        index["items"]
            .as_array()
            .expect("items")
            .iter()
            .any(|i| i["name"] == "private_helper")
    );
    assert!(
        index["edges"]
            .as_array()
            .expect("edges")
            .iter()
            .any(|e| e["kind"] == "facade" && e["target"] == "Tile")
    );
    assert!(
        index["edges"]
            .as_array()
            .expect("edges")
            .iter()
            .any(|e| e["kind"] == "re-exports" && e["target"] == "PublicCost")
    );
    let audit = value(reports::comments(&s, PUBISH, true).expect("audit"));
    assert_eq!(audit["total"]["files"], 3);
    assert_eq!(audit["total"]["missing_header"], 1);
    assert_eq!(audit["total"]["uncited_header"], 1);
    assert_eq!(audit["total"]["items"], 4);
    assert_eq!(audit["total"]["undocumented"], 2);
}
#[test]
fn documentation_and_string_spellings_do_not_make_edges_but_fields_do() {
    let f = Fixture::new();
    f.write("a/src/grade.rs", "pub struct Cost;\n");
    f.write("a/src/admit.rs","/// Cost\npub struct Prose {\n    // Cost\n    text: &'static str,\n}\npub const TEXT: &str = \"Cost\";\npub struct Field { cost: crate::grade::Cost }\n");
    let s = f.snapshot();
    let n = Navigation::new(&s, PUBISH.selection()).expect("navigation");
    let report = n.dump().expect("dump");
    let edges = presentation_edges(&report.edges);
    assert!(
        edges
            .iter()
            .any(|e| e.source_name == "Field" && e.target == "Cost")
    );
    assert!(
        !edges
            .iter()
            .any(|e| e.source_name == "Prose" && e.target == "Cost")
    );
    assert!(
        !edges
            .iter()
            .any(|e| e.source_name == "TEXT" && e.target == "Cost")
    );
}
#[test]
fn external_shadows_do_not_become_workspace_edges_and_real_ambiguity_remains() {
    let f = Fixture::new();
    f.write(
        "a/src/admit.rs",
        "use external::Cost;\npub fn produce()->Cost {}\n",
    );
    let s = f.snapshot();
    let n = Navigation::new(&s, PUBISH.selection()).expect("navigation");
    let index = n.dump().expect("dump");
    assert!(
        index
            .edges
            .iter()
            .any(|e| matches!(e.resolution, Resolution::Unresolved(_)))
    );
    assert_eq!(presentation_edges(&index.edges).len(), 0);
}
#[test]
fn union_is_a_counted_declaration() {
    let f = Fixture::new();
    f.write(
        "a/src/admit.rs",
        "pub union Word { pub integer:u64, pub real:f64 }\n",
    );
    let s = f.snapshot();
    let n = Navigation::new(&s, PUBISH.selection()).expect("navigation");
    assert_eq!(
        value(module_card(&n, "a/src/admit.rs", PUBISH, true, false, true).expect("module"))["census"]
            ["types"],
        1
    );
}
#[test]
fn historical_reads_keep_current_membership_and_exact_revision() {
    let f = Fixture::new();
    f.commit();
    let initial = f.git(&["rev-parse", "HEAD"]);
    let current = f.snapshot();
    f.write("a/src/admit.rs", "pub struct New;\n");
    f.commit();
    let old = Revision::read(f.root(), initial.trim()).expect("revision");
    let old = input::historical(&current, &old).expect("historical");
    assert!(
        old.source("a/src/admit.rs")
            .expect("source")
            .contains("struct Tile")
    );
    assert!(
        !old.source("a/src/admit.rs")
            .expect("source")
            .contains("struct New")
    );
    let new = Revision::read(f.root(), "HEAD").expect("revision");
    let new = input::historical(&current, &new).expect("historical");
    assert!(
        new.source("a/src/admit.rs")
            .expect("source")
            .contains("struct New")
    );
}
#[test]
fn shape_change_uses_physical_production_context_but_navigation_keeps_both() {
    let f = Fixture::new();
    f.write(
        "Cargo.toml",
        "[workspace]\nmembers=[\"a\",\"a/src/nested\"]\nresolver=\"3\"\n",
    );
    f.write(
        "a/Cargo.toml",
        "[package]\nname=\"z\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
    );
    f.write(
        "a/src/lib.rs",
        "#[cfg(test)]\n#[path=\"nested/src/lib.rs\"]\nmod nested;\n",
    );
    f.write(
        "a/src/nested/Cargo.toml",
        "[package]\nname=\"nested\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
    );
    f.write("a/src/nested/src/lib.rs", "pub struct Shared;\n");
    f.commit();
    f.write(
        "a/src/nested/src/lib.rs",
        "pub struct Shared;\npub fn added() {}\n",
    );
    let s = f.snapshot();
    let path = "a/src/nested/src/lib.rs";
    let files: Vec<_> = s
        .files(FileSelection::SourceTree)
        .filter(|file| file.path == path)
        .collect();
    assert_eq!(files.len(), 2);
    assert_eq!(files.iter().filter(|file| file.test_only).count(), 1);
    let n = Navigation::new(&s, PUBISH.selection()).expect("navigation");
    assert_eq!(n.module(path).expect("module").contexts.len(), 2);
    let delta = value(reports::diff(&s, "HEAD", true).expect("diff"));
    assert!(
        delta["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .any(|r| r["module"] == path && r["top_level_items"] == json!([1, 2]))
    );
}
#[test]
fn named_test_paths_and_production_mounts_select_the_native_population() {
    let f = Fixture::new();
    f.write(
        "a/src/lib.rs",
        "#[cfg(test)]\n#[path=\"shared.rs\"]\nmod checks;\n#[path=\"shared.rs\"]\nmod live;\n",
    );
    f.write("a/src/shared.rs", "pub struct Shared;\n");
    let s = f.snapshot();
    assert!(
        !s.physical_file("a/src/shared.rs")
            .expect("shared")
            .test_only
    );
    f.write(
        "a/src/lib.rs",
        "#[cfg(test)]\n#[path=\"shared.rs\"]\nmod checks;\n",
    );
    let s = f.snapshot();
    assert!(s.physical_file("a/src/shared.rs").expect("test").test_only);
    f.write("a/src/lib.rs", "#[path=\"shared.rs\"]\nmod live;\n");
    let s = f.snapshot();
    assert!(
        !s.physical_file("a/src/shared.rs")
            .expect("production")
            .test_only
    );
}
#[test]
fn unicode_source_and_test_regions_keep_source_order() {
    let text = "// λ and 中文\npub fn first() {}\n#[cfg(test)]\nmod tests {\n fn test_only() {}\n}\npub fn last() {}\n";
    let source = atlas_core::source::Source::of(Path::new("src/lib.rs"), text.to_owned());
    let names: Vec<_> = source
        .outline()
        .iter()
        .filter(|i| !source.test_regions().covers(i.line))
        .map(|i| i.name.as_str())
        .collect();
    assert!(names.contains(&"first"));
    assert!(names.contains(&"last"));
    assert!(!names.contains(&"test_only"));
}

#[test]
fn unicode_in_live_code_preserves_projection_bytes() {
    let text = "const π: f64 = 3.14;\npub struct After;\n";
    let projection = atlas_core::source::RustSourceProjection::of(text.to_owned());
    assert_eq!(
        projection.code_lines().collect::<Vec<_>>(),
        text.lines().collect::<Vec<_>>()
    );
    assert!(
        projection
            .outline(Path::new("src/lib.rs"))
            .iter()
            .any(|item| item.name == "After")
    );
}

#[test]
fn orphan_names_keep_native_alphabetical_order() {
    let f = Fixture::new();
    f.write("a/src/lib.rs", "pub struct Z;\npub struct A;\n");
    let s = f.snapshot();
    let n = Navigation::new(&s, PUBISH.selection()).expect("navigation");
    let report =
        value(module_card(&n, "a/src/lib.rs", PUBISH, false, false, true).expect("module"));
    assert_eq!(report["orphans"], json!(["A", "Z"]));
}

#[test]
fn external_targets_do_not_enter_native_capture_or_shape_changes() {
    let f = Fixture::new();
    f.write("a/Cargo.toml", "[package]\nname=\"sample\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[[test]]\nname=\"external\"\npath=\"tests/external.rs\"\n");
    f.write("a/tests/external.rs", "fn external() {}\n");
    f.commit();
    f.write("a/tests/external.rs", "fn changed() {}\nfn another() {}\n");
    let s = f.snapshot();
    assert!(s.source("a/tests/external.rs").is_none());
    assert_eq!(input::diagnostic_lines(&s), Vec::<String>::new());
    assert_eq!(
        value(reports::diff(&s, "HEAD", true).expect("diff"))["rows"],
        json!([])
    );
}

#[cfg(unix)]
#[test]
fn excluded_symlink_reports_a_stable_witness_without_contributing_counts() {
    let f = Fixture::new();
    f.write("a/src/lib.rs", "pub struct Present;\n");
    f.write("a/omitted.rs", "pub struct Omitted;\n");
    let baseline = f.snapshot();
    let baseline_navigation = Navigation::new(&baseline, PUBISH.selection()).expect("baseline");
    let expected =
        value(census(&baseline_navigation, None, PUBISH, true).expect("baseline census"));
    std::os::unix::fs::symlink("../omitted.rs", f.root().join("a/src/link.rs")).expect("symlink");
    let s = f.snapshot();
    assert_eq!(
        input::diagnostic_lines(&s),
        vec!["atlas source: a/src/link.rs: symlink excluded from physical source walk"]
    );
    let n = Navigation::new(&s, PUBISH.selection()).expect("navigation");
    assert_eq!(
        value(census(&n, None, PUBISH, true).expect("census")),
        expected
    );
    assert!(s.source("a/src/link.rs").is_none());
}

#[test]
fn same_crate_edges_resolve_but_cross_crate_duplicates_stay_ambiguous() {
    let f = Fixture::new();
    f.write(
        "Cargo.toml",
        "[workspace]\nmembers=[\"a\",\"b\",\"c\"]\nresolver=\"3\"\n",
    );
    for name in ["b", "c"] {
        f.write(
            &format!("{name}/Cargo.toml"),
            &format!("[package]\nname=\"{name}\"\nversion=\"0.1.0\"\nedition=\"2024\"\n"),
        );
    }
    f.write("a/src/lib.rs", "pub struct Shared;\n");
    f.write(
        "b/src/lib.rs",
        "pub struct Shared;\npub fn use_shared(x: Shared) -> Shared { x }\n",
    );
    f.write("c/src/lib.rs", "pub fn probe(x: Shared) {}\n");
    let s = f.snapshot();
    let n = Navigation::new(&s, PUBISH.selection()).expect("navigation");
    let d = n.dump().expect("dump");
    let edges = presentation_edges(&d.edges);
    let local: Vec<_> = edges
        .iter()
        .filter(|e| e.source_name == "use_shared")
        .collect();
    assert_eq!(local.len(), 2);
    assert!(
        local
            .iter()
            .all(|e| !e.ambiguous && e.target_file.as_deref() == Some("b/src/lib.rs"))
    );
    let remote: Vec<_> = edges.iter().filter(|e| e.source_name == "probe").collect();
    assert_eq!(remote.len(), 1);
    assert!(remote[0].ambiguous && remote[0].target_file.is_none());
}

#[test]
fn fake_imports_cannot_suppress_real_edges_and_bounds_keep_return_classification() {
    let f = Fixture::new();
    f.write("a/src/lib.rs", "pub struct Value;\npub struct Tile;\npub struct Cost;\npub struct Grade;\npub struct Frame;\nmod user;\n");
    f.write("a/src/user.rs", "/* use external::Value; */\nconst SOURCE: &str = r#\"use external::Value;\"#;\npub fn encode() -> Value {}\npub fn map<F: Fn(Tile) -> Cost>(f:F, x:Tile) -> Grade {}\npub fn tick(frame:Frame) {}\n");
    let s = f.snapshot();
    let n = Navigation::new(&s, PUBISH.selection()).expect("navigation");
    let dump = n.dump().expect("dump");
    let edges = presentation_edges(&dump.edges);
    assert!(
        edges
            .iter()
            .any(|e| e.source_name == "encode" && e.target == "Value")
    );
    let map: Vec<_> = edges.iter().filter(|e| e.source_name == "map").collect();
    assert!(
        map.iter()
            .any(|e| class_label(e.class) == "consumes" && e.target == "Tile")
    );
    assert!(
        map.iter()
            .any(|e| class_label(e.class) == "produces" && e.target == "Grade")
    );
    assert!(
        !map.iter()
            .any(|e| class_label(e.class) == "produces" && e.target == "Cost")
    );
    assert!(edges.iter().any(|e| e.source_name == "tick"
        && e.target == "Frame"
        && class_label(e.class) == "consumes"));
}

#[test]
fn external_shadow_preserves_unrelated_local_edges() {
    let f = Fixture::new();
    f.write(
        "a/src/admit.rs",
        "use external::Cost;\npub struct Tile;\npub fn produce(x:Tile)->Cost {}\n",
    );
    let s = f.snapshot();
    let n = Navigation::new(&s, PUBISH.selection()).expect("navigation");
    let d = n.dump().expect("dump");
    let edges = presentation_edges(&d.edges);
    assert!(
        edges
            .iter()
            .any(|e| e.source_name == "produce" && e.target == "Tile")
    );
    assert!(
        !edges
            .iter()
            .any(|e| e.source_name == "produce" && e.target == "Cost")
    );
}

#[test]
fn each_native_citation_marker_and_missing_documentation_are_observable() {
    let f = Fixture::new();
    for marker in ["CR-D104", ".design/spec.md", "corpus-runtime", "§5"] {
        f.write("a/src/lib.rs", &format!("//! {marker}\npub struct Bare;\n"));
        let s = f.snapshot();
        let report = value(reports::comments(&s, PUBISH, true).expect("audit"));
        assert_eq!(report["total"]["uncited_header"], 0, "{marker}");
    }
    f.write("a/src/lib.rs", "pub struct Bare;\n");
    let s = f.snapshot();
    let n = Navigation::new(&s, PUBISH.selection()).expect("navigation");
    assert!(
        name_card(&n, "Bare", PUBISH, false)
            .expect("name")
            .contains("MISSING — no doc comment")
    );
    assert!(
        module_card(&n, "a/src/lib.rs", PUBISH, true, false, false)
            .expect("module")
            .contains("//! MISSING")
    );
    let d = value(dump(&n, &s, ALL).expect("dump"));
    let bare = d["items"]
        .as_array()
        .expect("items")
        .iter()
        .find(|i| i["name"] == "Bare")
        .expect("Bare");
    assert_eq!(bare["vis"], "pub");
    assert_eq!(bare["pub"], true);
    assert!(bare["id"].as_str().expect("id").ends_with(":1:Bare"));
}

#[test]
fn approximate_references_keep_identifier_boundaries() {
    let f = Fixture::new();
    let s = f.snapshot();
    let n = Navigation::new(&s, PUBISH.selection()).expect("navigation");
    let before = n.name("Tile").expect("before").refs_approx;
    f.write(
        "a/src/words.rs",
        "fn words() { let _: Tile; let _: LaneTile; Tile::new(); let _: TileSet; let _: Tile_x; let _ = (Tile); }\n",
    );
    let s = f.snapshot();
    let n = Navigation::new(&s, PUBISH.selection()).expect("navigation");
    assert_eq!(n.name("Tile").expect("after").refs_approx, before + 3);
}

#[test]
fn historical_source_walk_does_not_require_a_later_target() {
    let f = Fixture::new();
    f.commit();
    f.write("a/Cargo.toml","[package]\nname=\"sample\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[[bin]]\nname=\"later\"\npath=\"src/later.rs\"\n");
    f.write("a/src/later.rs", "fn main() {}\n");
    let s = f.snapshot();
    let revision = Revision::read(f.root(), "HEAD").expect("revision");
    let old = input::historical(&s, &revision).expect("historical input");
    assert!(old.source("a/src/later.rs").is_none());
    assert_eq!(input::diagnostic_lines(&old), Vec::<String>::new());
    let delta = value(reports::diff(&s, "HEAD", true).expect("diff"));
    assert!(
        delta["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .any(|r| r["module"] == "a/src/later.rs")
    );
}

#[test]
fn grouped_external_bindings_and_nested_module_names_keep_native_meaning() {
    for (declaration, expected) in [
        ("use serde_json::value::Value;", vec!["Value"]),
        (
            "use serde_json::{Map, Value as JsonValue};",
            vec!["JsonValue", "Map"],
        ),
        (
            "use serde_json::{self, Value};",
            vec!["Value", "serde_json"],
        ),
        ("use serde_json::*;", vec!["*"]),
    ] {
        let mut names: Vec<_> = atlas_core::source::import_bindings(declaration)
            .into_iter()
            .map(|b| b.local_name)
            .collect();
        names.sort();
        assert_eq!(names, expected);
    }
    assert_eq!(
        atlas_core::discovery::module_path_of("a/src", "a/src/nested/leaf.rs").as_deref(),
        Some("nested::leaf")
    );
    assert_eq!(
        atlas_core::discovery::module_path_of("a/src", "a/src/nested/mod.rs").as_deref(),
        Some("nested")
    );
    assert_eq!(
        atlas_core::discovery::module_path_of("a/src", "a/src/lib.rs"),
        None
    );
    let f = Fixture::new();
    f.write("a/src/lib.rs", "pub mod nested;\n");
    f.write("a/src/nested/mod.rs", "pub struct ViaMod;\npub mod leaf;\n");
    f.write("a/src/nested/leaf.rs", "pub struct ViaFile;\n");
    let s = f.snapshot();
    let n = Navigation::new(&s, PUBISH.selection()).expect("navigation");
    assert_eq!(
        n.name("nested::mod::ViaMod").expect("mod.rs").matches.len(),
        1
    );
    assert_eq!(
        n.name("nested::leaf::ViaFile").expect("file").matches.len(),
        1
    );
}
