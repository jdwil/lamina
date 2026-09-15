//! Error types for the Lamina engine.

use thiserror::Error;

/// An error produced while lexing or parsing Lamina source.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ParseError {
    /// The lexer encountered a character it does not recognize.
    #[error("unexpected character {ch:?} at byte offset {offset}")]
    UnexpectedChar {
        /// The offending character.
        ch: char,
        /// Byte offset into the source where it occurred.
        offset: usize,
    },

    /// The parser expected a specific token but found something else (or end of
    /// input).
    #[error("expected {expected} but found {found}")]
    Expected {
        /// A human-readable description of what was expected.
        expected: String,
        /// A human-readable description of what was actually found.
        found: String,
    },

    /// A type name was used that is not a known Lamina primitive (for this
    /// slice).
    #[error("unknown type {name:?}")]
    UnknownType {
        /// The unrecognized type name.
        name: String,
    },
}

/// An error produced while parsing a rigid `.mdl` language-definition document.
///
/// The format is markdown-compatible on the surface but strictly validated: a
/// missing or malformed required element is a hard error, never silently
/// ignored. Prose between sections is documentation and is not validated.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum LangDocError {
    /// The document did not begin with the required title line
    /// `# Lamina Language Definition: <name>`.
    #[error("expected title line `# Lamina Language Definition: <name>` on line 1, found {found:?}")]
    MissingTitle {
        /// The actual first non-empty line (or empty string if the document was
        /// empty).
        found: String,
    },

    /// A required section (identified by its `##` heading) was absent.
    #[error("missing required section {heading:?}")]
    MissingSection {
        /// The heading of the required section.
        heading: String,
    },

    /// A required fenced config block (identified by its language tag) was
    /// absent.
    #[error("missing required config block ```{tag}```")]
    MissingBlock {
        /// The expected fenced-block language tag.
        tag: String,
    },

    /// A fenced config block was opened but never closed with a matching fence.
    #[error("unterminated config block ```{tag}```")]
    UnterminatedBlock {
        /// The fenced-block language tag that was left open.
        tag: String,
    },

    /// A required key was absent from a config block.
    #[error("missing required key {key:?} in block ```{tag}```")]
    MissingKey {
        /// The expected key name.
        key: String,
        /// The block the key was expected in.
        tag: String,
    },

    /// A line inside a config block did not match the expected shape.
    #[error("malformed line in block ```{tag}```: {line:?}")]
    MalformedLine {
        /// The block tag.
        tag: String,
        /// The offending line.
        line: String,
    },

    /// A boolean-valued key had a value other than `true` or `false`.
    #[error("expected `true` or `false` for key {key:?}, found {value:?}")]
    InvalidBool {
        /// The key name.
        key: String,
        /// The invalid value.
        value: String,
    },

    /// A string value was not properly double-quoted.
    #[error("expected a double-quoted string for key {key:?}, found {value:?}")]
    UnquotedString {
        /// The key name.
        key: String,
        /// The invalid value.
        value: String,
    },

    /// A capability line referenced a primitive the engine does not know.
    #[error("unknown primitive {name:?} in capability matrix")]
    UnknownPrimitive {
        /// The unrecognized primitive spelling.
        name: String,
    },

    /// A capability line used an action that is not one of `identity`, `alias`,
    /// `widen`, or `forbid`.
    #[error("unknown capability action {action:?} (expected identity, alias, widen, or forbid)")]
    UnknownAction {
        /// The unrecognized action.
        action: String,
    },

    /// A non-`forbid` capability action was missing its required target type.
    #[error("capability action {action:?} for {primitive:?} requires a target type")]
    MissingCapabilityTarget {
        /// The action that requires a target.
        action: String,
        /// The primitive the action applied to.
        primitive: String,
    },
}

/// An error produced while emitting target-language source.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum EmitError {
    /// The program uses a primitive the target language definition forbids.
    #[error("target {target:?} forbids primitive {primitive:?}")]
    ForbiddenPrimitive {
        /// The name of the target language definition.
        target: String,
        /// The forbidden primitive's Lamina spelling.
        primitive: String,
    },
}
