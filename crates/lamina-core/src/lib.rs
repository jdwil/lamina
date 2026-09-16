//! # lamina-core
//!
//! The Lamina engine. Lamina is a review-oriented intermediate representation:
//! it *lowers* to native source code and (in future slices) *raises* to
//! human-reviewable artifacts.
//!
//! This crate implements the core equation:
//!
//! ```text
//! Lamina source + language definition = target source
//! ```
//!
//! The engine is deliberately minimal ("dumb"): it understands a frozen set of
//! kernel primitives and keywords, but all target-specific behavior lives in a
//! [`lang::LanguageDef`]. To support a new target language you supply a new
//! language definition, never new engine logic.
//!
//! The first end-to-end slice parses a single function returning an integer
//! literal and transpiles it to Rust and TypeScript. Everything else is built
//! additively on this skeleton.

pub mod ast;
pub mod emitter;
pub mod error;
pub mod lang;
pub mod lang_doc;
pub mod lexer;
pub mod parser;
pub mod predicate;
pub mod render;

use std::path::Path;

use thiserror::Error;

pub use error::{EmitError, LangDocError, ParseError, PredicateError};

/// An error from loading a language-definition document from disk.
#[derive(Debug, Error)]
pub enum LoadError {
    /// The definition file could not be read.
    #[error("failed to read language definition {path:?}: {source}")]
    Io {
        /// The path that could not be read.
        path: String,
        /// The underlying I/O error.
        source: std::io::Error,
    },
    /// The definition document was malformed.
    #[error("invalid language definition {path:?}: {source}")]
    Parse {
        /// The path that failed to parse.
        path: String,
        /// The underlying parse error.
        source: LangDocError,
    },
}

/// Loads and parses a language-definition document from `path` into a
/// [`lang::LanguageDef`].
///
/// This is the explicit-path load — the last resort of the future resolution
/// waterfall (CLI arg -> local dir -> `languages/` -> registry). The engine
/// itself ships no built-in definitions; every definition is loaded from a
/// document like this.
///
/// # Errors
///
/// Returns [`LoadError`] if the file cannot be read or the document is
/// malformed.
pub fn load_language_def(path: &Path) -> Result<lang::LanguageDef, LoadError> {
    let src = std::fs::read_to_string(path).map_err(|source| LoadError::Io {
        path: path.display().to_string(),
        source,
    })?;
    lang_doc::parse_language_def(&src).map_err(|source| LoadError::Parse {
        path: path.display().to_string(),
        source,
    })
}

/// An error from the full parse-then-emit pipeline.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TranspileError {
    /// Parsing the Lamina source failed.
    #[error("parse error: {0}")]
    Parse(#[from] ParseError),
    /// Emitting the target source failed.
    #[error("emit error: {0}")]
    Emit(#[from] EmitError),
}

/// Parses Lamina inner-code `src` and transpiles it to the target described by
/// `lang`.
///
/// This is the convenience entry point wrapping [`parser::parse`] and
/// [`emitter::emit`].
///
/// # Errors
///
/// Returns [`TranspileError`] if parsing or emission fails.
pub fn transpile(src: &str, lang: &lang::LanguageDef) -> Result<String, TranspileError> {
    let file = parser::parse(src)?;
    let output = emitter::emit(&file, lang)?;
    Ok(output)
}
