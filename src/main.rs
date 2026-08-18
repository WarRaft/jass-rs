use std::env;
use std::fs;
use std::process::ExitCode;

use jass_rs::{lint, Level};

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let path = match args.next() {
        Some(path) => path,
        None => {
            eprintln!("usage: jass-rs <file.j>");
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

    let program = match jass_rs::parse(&src) {
        Ok(program) => program,
        Err(err) => {
            eprintln!("{path}:{}:{}: error: {}", err.line, err.col, err.message);
            return ExitCode::FAILURE;
        }
    };

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
