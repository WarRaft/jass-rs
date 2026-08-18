use jass_rs::{parse, render_tree};
use std::fs;
use std::path::Path;

const SAMPLE: &str = include_str!("fixtures/sample.j");

/// This test renders `sample.j`'s AST as a tree and compares it against a
/// checked-in golden file. It exists so that a parser change's effect on the
/// AST is visible in the diff of a plain-text file, instead of having to
/// step through the parser or read raw `Debug` output.
///
/// When a change to the parser is intentional, regenerate the golden file
/// with:
///
/// ```sh
/// UPDATE_SNAPSHOTS=1 cargo test --test ast_snapshot
/// ```
///
/// then review the diff of `tests/fixtures/sample.ast.txt` like any other
/// code change.
#[test]
fn ast_dump_matches_snapshot() {
    let program = parse(SAMPLE).expect("sample.j should parse");
    let actual = render_tree(&program);
    let snapshot_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample.ast.txt");

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
        "AST dump for sample.j changed — if this is expected, regenerate with \
         `UPDATE_SNAPSHOTS=1 cargo test --test ast_snapshot` and review the diff"
    );
}
