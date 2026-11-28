use std::env;
use std::fs;
use std::process::ExitCode;

use jass_rs::{lint, render_dot, render_html, render_tree, Dialect, Level};

enum Mode {
    Lint,
    Ast,
    Dot,
    Html,
}

fn usage() {
    eprintln!("usage: jass-rs [--jass|--vjass] [--ast|--dot|--html] <file.j>");
    eprintln!("       jass-rs --strict [--with common.j]... <file.j>...");
    eprintln!("  (no flag)  lint the file and print diagnostics");
    eprintln!("  --ast      print the parsed AST as an indented tree");
    eprintln!("  --dot      print the parsed AST as a Graphviz DOT graph");
    eprintln!("  --html     print a self-contained AST viewer page (source + tree)");
    eprintln!("  --jass     force plain JASS");
    eprintln!("  --vjass    force vJASS (embedded `//! zinc` blocks)");
    eprintln!("             (default: the file's `// @dialect` header, else plain JASS)");
    eprintln!("  --strict   check the files as the game reads a map's script: one file,");
    eprintln!("             glued in the order given (a builder of one's own), strict JASS;");
    eprintln!("             a function called above its declaration, and the rest of");
    eprintln!("             the group `strict`, are errors");
    eprintln!("  --with F   (with --strict) the game's declarations (common.j, Blizzard.j):");
    eprintln!("             before the script, not part of it; names are checked over them");
    eprintln!();
    eprintln!("  e.g. jass-rs --html file.j > file.ast.html && open file.ast.html");
    eprintln!("       jass-rs --strict --with common.j --with Blizzard.j head.j lib.j main.j");
}

/// `--strict`: `files` as one script, over `with`'s declarations.
fn strict(files: &[String], with: &[String]) -> ExitCode {
    let read = |p: &String| fs::read_to_string(p).map_err(|e| eprintln!("{p}: {e}")).ok();
    let Some(texts) = files.iter().map(read).collect::<Option<Vec<String>>>() else { return ExitCode::FAILURE };
    let Some(env_texts) = with.iter().map(read).collect::<Option<Vec<String>>>() else { return ExitCode::FAILURE };
    let mut env = Vec::new();
    for (p, t) in with.iter().zip(&env_texts) {
        match jass_rs::parse(t) {
            Ok(program) => env.push(jass_rs::names::Declarations::of(&program)),
            Err(e) => {
                eprintln!("{p}:{}:{}: error: {}", e.line, e.col, e.message);
                return ExitCode::FAILURE;
            }
        }
    }
    let env: Vec<&jass_rs::names::Declarations> = env.iter().collect();
    let sources: Vec<(&str, &str)> = files.iter().map(String::as_str).zip(texts.iter().map(String::as_str)).collect();
    let found = jass_rs::strict::check(&sources, &env);
    for f in &found {
        let d = &f.diagnostic;
        let level = if d.level == Level::Error { "error" } else { "warning" };
        let rule = if d.rule.is_empty() { String::new() } else { format!(" [{}]", d.rule) };
        println!("{}:{}: {level}: {}{rule}", files[f.file], d.line, d.message);
    }
    if found.is_empty() {
        println!("{}: no issues found", files.join(" + "));
    }
    if found.iter().any(|f| f.diagnostic.level == Level::Error) { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}

fn main() -> ExitCode {
    // `--strict`: every other argument a file, in order; `--with F` the game's.
    let args: Vec<String> = env::args().skip(1).collect();
    if args.iter().any(|a| a == "--strict") {
        let (mut files, mut with) = (Vec::new(), Vec::new());
        let mut it = args.iter().filter(|a| *a != "--strict");
        while let Some(a) = it.next() {
            match a.as_str() {
                "--with" => match it.next() {
                    Some(f) => with.push(f.clone()),
                    None => {
                        usage();
                        return ExitCode::FAILURE;
                    }
                },
                _ => files.push(a.clone()),
            }
        }
        if files.is_empty() {
            usage();
            return ExitCode::FAILURE;
        }
        return strict(&files, &with);
    }
    let mut mode = Mode::Lint;
    let mut dialect = None;
    let mut path = None;
    for arg in env::args().skip(1) {
        match arg.as_str() {
            "--ast" => mode = Mode::Ast,
            "--dot" => mode = Mode::Dot,
            "--html" => mode = Mode::Html,
            "--jass" => dialect = Some(Dialect::Jass),
            "--vjass" => dialect = Some(Dialect::VJass),
            "-h" | "--help" => {
                usage();
                return ExitCode::SUCCESS;
            }
            other => path = Some(other.to_string()),
        }
    }

    let path = match path {
        Some(path) => path,
        None => {
            usage();
            return ExitCode::FAILURE;
        }
    };

    let src = match fs::read_to_string(&path) {
        Ok(src) => src,
        Err(err) => {
            eprintln!("{path}: {err}");
            return ExitCode::FAILURE;
        }
    };

    let dialect = dialect.unwrap_or_else(|| Dialect::detect(&src));
    let program = match jass_rs::parse_with(&src, dialect) {
        Ok(program) => program,
        Err(err) => {
            eprintln!("{path}:{}:{}: error: {}", err.line, err.col, err.message);
            return ExitCode::FAILURE;
        }
    };

    match mode {
        Mode::Ast => {
            print!("{}", render_tree(&program));
            ExitCode::SUCCESS
        }
        Mode::Dot => {
            print!("{}", render_dot(&program));
            ExitCode::SUCCESS
        }
        Mode::Html => {
            print!("{}", render_html(&program, &src, &path));
            ExitCode::SUCCESS
        }
        Mode::Lint => {
            let diagnostics = lint(&program);
            let mut has_error = false;
            for diag in &diagnostics {
                let level = match diag.level {
                    Level::Error => {
                        has_error = true;
                        "error"
                    }
                    Level::Warning => "warning",
                };
                println!("{path}:{}: {level}: {}", diag.line, diag.message);
            }

            if diagnostics.is_empty() {
                println!("{path}: no issues found");
            }

            if has_error {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
    }
}
