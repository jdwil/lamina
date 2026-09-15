//! The Lamina command-line interface.
//!
//! ```text
//! lamina <source-file> --lang <definition.mdl>
//! ```
//!
//! `<source-file>` contains Lamina inner-code (the strict grammar, not yet the
//! `.mdl` document wrapper). `<definition.mdl>` is a rigid language-definition
//! document (see `lamina_core::lang_doc`). The transpiled target source is
//! printed to stdout.
//!
//! Passing the definition explicitly via `--lang` is the last resort of the
//! planned resolution waterfall (CLI arg -> local dir -> `languages/` ->
//! registry); the engine ships no built-in definitions.

use std::path::Path;
use std::process::ExitCode;

use lamina_core::{load_language_def, transpile};

struct Args {
    source: String,
    lang: String,
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Some(args) => args,
        None => {
            eprintln!("usage: lamina <source-file> --lang <definition.mdl>");
            return ExitCode::FAILURE;
        }
    };

    let lang = match load_language_def(Path::new(&args.lang)) {
        Ok(lang) => lang,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::FAILURE;
        }
    };

    let src = match std::fs::read_to_string(&args.source) {
        Ok(src) => src,
        Err(err) => {
            eprintln!("failed to read {:?}: {err}", args.source);
            return ExitCode::FAILURE;
        }
    };

    match transpile(&src, &lang) {
        Ok(output) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("transpile failed: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Parses `<source-file> --lang <definition.mdl>` in either order for the flag.
fn parse_args() -> Option<Args> {
    let mut source: Option<String> = None;
    let mut lang: Option<String> = None;
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--lang" | "-l" => {
                lang = Some(args.next()?);
            }
            positional if !positional.starts_with('-') && source.is_none() => {
                source = Some(positional.to_string());
            }
            _ => return None,
        }
    }

    Some(Args {
        source: source?,
        lang: lang?,
    })
}
