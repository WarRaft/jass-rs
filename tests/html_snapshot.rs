use jass_rs::{parse, render_html};
use std::fs;
use std::path::{Path, PathBuf};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Renders every `tests/fixtures/*.j` file's AST as the self-contained HTML
/// viewer and compares it against a `<name>.ast.html` golden file sitting
/// next to it. The snapshot's file name is derived from the fixture's own
/// name, so adding, renaming, or removing a `.j` fixture just works —
/// there's nothing here to update by hand.
///
/// When a change to the parser or the viewer template is intentional,
/// regenerate every golden file with:
///
/// ```sh
/// UPDATE_SNAPSHOTS=1 cargo test --test html_snapshot
/// ```
///
/// then review the diffs like any other code change, and open one of the
/// `.ast.html` files in a browser to sanity-check it visually.
#[test]
fn html_dump_matches_snapshot() {
    let dir = fixtures_dir();
    let update = std::env::var_os("UPDATE_SNAPSHOTS").is_some();

    let mut fixtures: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", dir.display()))
        .map(|entry| entry.expect("failed to read dir entry").path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("j"))
        .collect();
    fixtures.sort();
    assert!(
        !fixtures.is_empty(),
        "no .j fixtures found in {}",
        dir.display()
    );

    let mut mismatches = Vec::new();
    for source_path in fixtures {
        let file_name = source_path
            .file_name()
            .and_then(|n| n.to_str())
            .expect("fixture file name should be valid UTF-8")
            .to_string();
        let source = fs::read_to_string(&source_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", source_path.display()));
        let program = parse(&source)
            .unwrap_or_else(|e| panic!("{} should parse: {e:?}", source_path.display()));
        let actual = render_html(&program, &source, &file_name);
        let snapshot_path = source_path.with_extension("ast.html");

        if update {
            fs::write(&snapshot_path, &actual)
                .unwrap_or_else(|e| panic!("failed to write {}: {e}", snapshot_path.display()));
            continue;
        }

        let expected = fs::read_to_string(&snapshot_path).unwrap_or_else(|_| {
            panic!(
                "missing snapshot at {}; run with UPDATE_SNAPSHOTS=1 to create it",
                snapshot_path.display()
            )
        });
        if actual != expected {
            mismatches.push(file_name);
        }
    }

    assert!(
        mismatches.is_empty(),
        "HTML dump changed for: {} — if this is expected, regenerate with \
         `UPDATE_SNAPSHOTS=1 cargo test --test html_snapshot` and review the diff",
        mismatches.join(", ")
    );
}
