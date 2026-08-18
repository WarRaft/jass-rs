use jass_rs::{parse, render_dot};
use std::fs;
use std::path::Path;

const SAMPLE: &str = include_str!("fixtures/sample.j");

/// Renders `sample.j`'s AST as a Graphviz DOT graph and compares it against
/// a checked-in golden file, mirroring `ast_snapshot.rs`. Regenerate a
/// picture from the checked-in file with:
///
/// ```sh
/// dot -Tsvg tests/fixtures/sample.dot -o /tmp/sample.svg && open /tmp/sample.svg
/// ```
///
/// When a parser change is intentional, regenerate the golden file with:
///
/// ```sh
/// UPDATE_SNAPSHOTS=1 cargo test --test dot_snapshot
/// ```
#[test]
fn dot_dump_matches_snapshot() {
    let program = parse(SAMPLE).expect("sample.j should parse");
    let actual = render_dot(&program);
    let snapshot_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample.dot");

    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        fs::write(&snapshot_path, &actual).expect("failed to write snapshot");
        return;
    }

    let expected = fs::read_to_string(&snapshot_path).unwrap_or_else(|_| {
        panic!(
            "missing snapshot at {}; run with UPDATE_SNAPSHOTS=1 to create it",
            snapshot_path.display()
        )
    });
    assert_eq!(
        actual, expected,
        "DOT dump for sample.j changed — if this is expected, regenerate with \
         `UPDATE_SNAPSHOTS=1 cargo test --test dot_snapshot` and review the diff"
    );
}
