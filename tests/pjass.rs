//! pjass's own tests (`../pjass-master/tests`, or `PJASS_TESTS`): what
//! pjass passes, strict JASS passes; what it fails, strict JASS fails, but
//! for the differences decided (`DIFFERENT`). A test of one of pjass's
//! flags (`//# +flag`) fails, or passes, by that flag's rule here.
//! Skipped when the folder isn't there.

use jass_rs::{names, strict, Level};
use std::path::{Path, PathBuf};

/// Where strict JASS reads otherwise than pjass, and why.
const DIFFERENT: &[(&str, &str)] = &[
    // A local is, to the game, a parameter on its stack, found by name
    // from the top: the last of a name is found first, an earlier one
    // unreachable. Not stopping the compiling, not an error: a typo, a
    // warning (`duplicate-declaration`; user, 2026-11-28).
    ("should-fail/shadowing/shadowing-3.j", "a parameter and a local of one name: legal, the last one's taken"),
    ("should-fail/shadowing/shadow-array-2.j", "a parameter and a local of one name: legal, the last one's taken"),
    ("should-fail/flags/shadowing-3.j", "a parameter and a local of one name: legal, the last one's taken"),
    // A byte 0xFF in a comment or a string: the text is read as UTF-8, a
    // byte that isn't is lost to it (U+FFFD; a rawcode's are found again).
    ("should-fail/syntax/max-byte-in-comment.j", "a byte that isn't UTF-8 is lost to the text"),
    ("should-fail/syntax/max-byte-in-string.j", "a byte that isn't UTF-8 is lost to the text"),
    // A rawcode has no escapes: `\` before a line break is a byte.
    ("should-fail/rawcodes/rawcodelit-19.j", "a rawcode has no escapes"),
    // A rawcode has no escapes (user, 2026-11-28): its bytes up to the
    // closing quote, one or four; `\\ab` four.
    ("should-fail/rawcodes/rawcodelit-10.j", "a rawcode has no escapes: four bytes"),
    ("should-fail/rawcodes/rawcodelit-11.j", "a rawcode has no escapes: four bytes"),
    ("should-fail/rawcodes/rawcodelit-12.j", "a rawcode has no escapes: four bytes"),
    // Any byte: one an editor can't keep is `unsafe-rawcode`'s, a warning
    // whose fix writes it as `0x...`.
    ("should-fail/syntax/max-byte-in-rawcode.j", "any byte, warned of (`unsafe-rawcode`)"),
    // Not UTF-8: read here as the text it is, its bytes not counted.
    ("should-check/syntax/rawcode.j", "a rawcode of bytes that aren't UTF-8"),
    // The return bug (`+rb`) was closed in 1.24: not read.
    ("should-check/flags/rb-annotation-1.j", "the return bug isn't allowed"),
    ("should-check/flags/rb-annotation-2.j", "the return bug isn't allowed"),
    ("should-check/flags/exploit-1.j", "the return bug isn't allowed"),
    ("should-check/flags/exploit-2.j", "the return bug isn't allowed"),
    // `+nosyntaxerror`: errors are errors.
    ("should-check/flags/absolute-garbage.j", "errors aren't switched off"),
];

/// Each flag's rule.
const FLAGS: &[(&str, &str)] = &[
    ("filter", "filter-returns-boolean"),
    ("nomodulooperator", "modulo-operator"),
    ("checknumberliterals", "number-overflow"),
    ("shadow", "shadowed-global"),
    ("shadow", "shadowed-name"),
    ("checkstringlength", "long-string"),
    ("checkstringhash", "string-hash-collision"),
    ("checkglobalsinit", "global-maybe-uninitialized"),
    ("checklongnames", "long-name"),
];

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            files(&p, out);
        } else if p.extension().is_some_and(|x| x == "j") {
            out.push(p);
        }
    }
}

/// Every problem of `text` read as strict JASS, every rule on: (level, rule).
fn problems(text: &str) -> Vec<(Level, &'static str, String)> {
    let found = strict::check(&[("x.j", text)], &[]);
    let mut out: Vec<_> = found.into_iter().map(|f| (f.diagnostic.level, f.diagnostic.rule, f.diagnostic.message)).collect();
    // The rules off by default too, for the flags'.
    if let Ok(p) = jass_rs::parse(text) {
        let options = jass_rs::lint::Options { source: Some(text), ..Default::default() };
        out.extend(jass_rs::lint_with(&p, &options).into_iter().chain(names::check(&p, &[])).map(|d| (d.level, d.rule, d.message)));
    }
    out
}

#[test]
fn pjass_tests() {
    let dir = std::env::var("PJASS_TESTS").map(PathBuf::from).unwrap_or_else(|_| Path::new(env!("CARGO_MANIFEST_DIR")).join("../pjass-master/tests"));
    if !dir.is_dir() {
        eprintln!("no pjass tests at {}: skipped", dir.display());
        return;
    }
    let mut all = Vec::new();
    files(&dir.join("should-check"), &mut all);
    files(&dir.join("should-fail"), &mut all);
    let mut wrong = Vec::new();
    let mut checked = 0;
    for path in all {
        let name = path.strip_prefix(&dir).unwrap().to_string_lossy().replace('\\', "/");
        // pjass crashing: not a reading of a script.
        if name.starts_with("should-fail/crashes/") || DIFFERENT.iter().any(|(d, _)| *d == name) {
            continue;
        }
        let text = String::from_utf8_lossy(&std::fs::read(&path).unwrap()).into_owned();
        let should_fail = name.starts_with("should-fail/");
        // `+flag` on, `-flag` off again.
        let mut flags: Vec<&str> = Vec::new();
        for f in text.lines().filter_map(|l| l.trim().strip_prefix("//#")).flat_map(|l| l.split_whitespace()) {
            let (on, name) = match f.split_at_checked(1) {
                Some(("+", n)) => (true, n),
                Some(("-", n)) => (false, n),
                _ => continue,
            };
            for (_, r) in FLAGS.iter().filter(|(flag, _)| *flag == name) {
                flags.retain(|x| x != r);
                if on {
                    flags.push(r);
                }
            }
        }
        let found = problems(&text);
        let failed = if name.contains("/flags/") && !flags.is_empty() {
            found.iter().any(|(_, r, _)| flags.contains(r) || *r == "" || jass_rs::strict::in_strict_group(r))
                || found.iter().any(|(l, r, _)| *l == Level::Error && !r.is_empty() && *r != "unused-local")
        } else {
            found.iter().any(|(l, _, _)| *l == Level::Error)
        };
        checked += 1;
        if failed != should_fail {
            wrong.push(format!("{name}: {}", if should_fail { "passes".to_string() } else { format!("fails: {:?}", found.iter().filter(|f| f.0 == Level::Error).collect::<Vec<_>>()) }));
        }
    }
    assert!(wrong.is_empty(), "{checked} checked, {} read otherwise than pjass:\n{}", wrong.len(), wrong.join("\n"));
    eprintln!("{checked} of pjass's tests read as pjass reads them");
}
