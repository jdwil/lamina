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
    #[error(
        "expected title line `# Lamina Language Definition: <name>` on line 1, found {found:?}"
    )]
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

    /// A required `When` table (identified by its `###` sub-heading) was absent.
    #[error("missing required table {name:?}")]
    MissingTable {
        /// The expected table name (sub-heading).
        name: String,
    },

    /// A `When` table row was malformed (not a two-column pipe row, or missing
    /// its quoted template).
    #[error("malformed row in table {table:?}: {line:?}")]
    MalformedRow {
        /// The table the row belongs to.
        table: String,
        /// The offending line.
        line: String,
    },

    /// A template value was not properly double-quoted.
    #[error("expected a double-quoted template in table {table:?}, found {value:?}")]
    UnquotedTemplate {
        /// The table the value belongs to.
        table: String,
        /// The invalid value.
        value: String,
    },

    /// A table's rows were empty or its final row was not `else` (a table must
    /// end with a catch-all).
    #[error("table {table:?} must have at least one row ending in `else`")]
    MissingElseRow {
        /// The offending table.
        table: String,
    },

    /// A template string within a table failed to parse.
    #[error("bad template in table {table:?}: {detail}")]
    BadTemplate {
        /// The table the template belongs to.
        table: String,
        /// A description of the template error.
        detail: String,
    },

    /// A `When` predicate within a table failed to parse or validate.
    #[error("bad predicate in table {table:?}: {detail}")]
    BadPredicate {
        /// The table the predicate belongs to.
        table: String,
        /// A description of the predicate error.
        detail: String,
    },

    /// A capability line referenced a primitive the engine does not know.
    #[error("unknown primitive {name:?} in capability matrix")]
    UnknownPrimitive {
        /// The unrecognized primitive spelling.
        name: String,
    },

    /// A capability line used an action that is not one of `identity`, `alias`,
    /// `widen`, `wrap`, or `forbid`.
    #[error(
        "unknown capability action {action:?} (expected identity, alias, widen, wrap, or forbid)"
    )]
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

    /// A capability row was malformed.
    #[error("malformed capability row: {line:?}")]
    MalformedCapabilityRow {
        /// The offending line.
        line: String,
    },

    /// A template referenced a slot that neither has a `### <slot>` subsection
    /// nor is an engine-provided terminal slot. Detected at load time.
    #[error("template references unknown slot {slot:?} (no subsection and not a terminal slot)")]
    UnknownSlotReference {
        /// The unresolved slot name.
        slot: String,
    },

    /// The capability matrix does not cover every kernel primitive. Every
    /// primitive in the frozen kernel set MUST have a row.
    #[error("capability matrix is incomplete; missing primitives: {missing}")]
    IncompleteCapabilityMatrix {
        /// Comma-separated list of the missing primitives' canonical spellings.
        missing: String,
    },

    /// An iterable slot (e.g. `params`, `body`) was referenced without defining
    /// its paired item-slot subsection (`param`, `statement`).
    #[error("collection slot {collection:?} requires an item subsection ### {item}")]
    MissingItemSlot {
        /// The collection slot referenced (e.g. `params`).
        collection: String,
        /// The required item slot subsection name (e.g. `param`).
        item: String,
    },

    /// An operator table row was malformed (fewer than two columns).
    #[error("malformed operator row: {line:?}")]
    MalformedOperatorRow {
        /// The offending line.
        line: String,
    },

    /// An operator table row named an operator that is not a kernel operator.
    #[error("unknown operator {name:?} in operator table")]
    UnknownOperator {
        /// The unrecognized operator machine name.
        name: String,
    },

    /// An operator table row used an action that is not `spell` or `forbid`.
    #[error("unknown operator action {action:?} (expected spell or forbid)")]
    UnknownOperatorAction {
        /// The unrecognized action.
        action: String,
    },

    /// A `spell` operator action was missing its required target spelling.
    #[error("operator action `spell` for {operator:?} requires a target spelling")]
    MissingOperatorTarget {
        /// The operator the action applied to.
        operator: String,
    },

    /// The required `lang-meta` header block (a ```` ```lang-meta ```` fenced
    /// block near the title) was absent. Every definition MUST declare one.
    #[error("missing required `lang-meta` header block (near the title)")]
    MissingLangMeta,

    /// A line inside the `lang-meta` block was not a `key: value` pair.
    #[error("malformed `lang-meta` line {line:?} (expected `key: value`)")]
    MalformedLangMetaLine {
        /// The offending line.
        line: String,
    },

    /// The `lang-meta` block used a key the engine does not recognize. The key
    /// set is closed: `lamina-format`, `target`, `target-version`.
    #[error("unknown `lang-meta` key {key:?} (expected lamina-format, target, or target-version)")]
    UnknownLangMetaKey {
        /// The unrecognized key.
        key: String,
    },

    /// A required `lang-meta` key was absent. All three (`lamina-format`,
    /// `target`, `target-version`) are required on every definition.
    #[error("missing required `lang-meta` key {key:?}")]
    MissingLangMetaKey {
        /// The absent key.
        key: String,
    },

    /// The `lamina-format` version string was not a well-formed
    /// `major.minor.patch` semver.
    #[error("malformed `lamina-format` version {value:?} (expected major.minor.patch)")]
    MalformedFormatVersion {
        /// The offending version string.
        value: String,
    },

    /// The definition requires a `.mdl` format version NEWER than the engine
    /// understands. The engine is backward-compatible (loads equal/older
    /// versions) but refuses to load a definition that needs a newer format.
    #[error(
        "definition requires Lamina format version {required} but this engine is {engine} \
         (upgrade the engine, or use a definition written for an older format)"
    )]
    FormatVersionTooNew {
        /// The minimum format version the definition declared.
        required: String,
        /// The engine's own format version ([`crate::lang::LAMINA_FORMAT_VERSION`]).
        engine: String,
    },
}

/// An error produced while parsing or validating a `When` predicate.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PredicateError {
    /// The predicate referenced a fact that is not in the engine's closed
    /// registry.
    #[error("unknown fact {fact:?} in When predicate (facts are a closed set)")]
    UnknownFact {
        /// The offending fact text.
        fact: String,
    },
    /// The predicate was syntactically malformed.
    #[error("malformed When predicate: {detail}")]
    Syntax {
        /// A human-readable description of the syntax problem.
        detail: String,
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

    /// A selected slot resolved to the `forbid` directive: the
    /// construct/visibility/modifier is not expressible in this target.
    #[error("target {target:?} forbids this construct")]
    ForbiddenConstruct {
        /// The name of the target language definition.
        target: String,
    },

    /// The program uses an operator the target language definition forbids
    /// (via the `## Operators` table), or a `null` literal on a target that
    /// forbids the `ptr` primitive (`null` follows `ptr`).
    #[error("target {target:?} forbids operator {operator:?}")]
    ForbiddenOperator {
        /// The name of the target language definition.
        target: String,
        /// The forbidden operator's canonical spelling (or `null`).
        operator: String,
    },

    /// A `When` table had no row whose predicate matched (a malformed language
    /// definition — a well-formed table ends in an `else` row).
    #[error("language definition {target:?} has no matching row in table {table:?}")]
    NoMatchingRow {
        /// The name of the target language definition.
        target: String,
        /// The name of the table that failed to match.
        table: String,
    },

    /// A template referenced a slot the emitter cannot resolve (neither a leaf
    /// slot nor a named slot table in the definition).
    #[error("language definition {target:?} references unknown slot {slot:?}")]
    UnknownSlot {
        /// The name of the target language definition.
        target: String,
        /// The unresolved slot name.
        slot: String,
    },

    /// The program contains a top-level item kind the target language
    /// definition has no `## <Item>` section for (so it cannot be rendered).
    #[error("target {target:?} has no definition for item kind {item:?}")]
    UnknownItem {
        /// The name of the target language definition.
        target: String,
        /// The item kind's canonical spelling (e.g. `struct`).
        item: String,
    },

    /// A template within the language definition was malformed.
    #[error("malformed template in language definition {target:?}: {detail}")]
    Template {
        /// The name of the target language definition.
        target: String,
        /// A human-readable description of the template problem.
        detail: String,
    },
}
