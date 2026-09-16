//! The language-definition model.
//!
//! A [`LanguageDef`] is the second half of Lamina's core equation:
//!
//! ```text
//! Lamina source + language definition = target source
//! ```
//!
//! The engine itself is deliberately "dumb": it knows the kernel primitives and
//! keywords, but every target-specific decision — how a primitive is spelled or
//! whether it is even allowed, and the surface syntax of a function — lives
//! here. This keeps a clean seam: to add a target, you add a language
//! definition, never engine logic.
//!
//! For the minimal slice a language definition carries:
//! - a **capability matrix**: how each kernel primitive is realized in the
//!   target (see [`Capability`]);
//! - **function emission** parameters: the surface syntax used to render a
//!   function.

use std::collections::HashMap;

use crate::ast::{ItemKind, Primitive};
use crate::predicate::{Predicate, RenderContext};
use crate::render::Template;

/// How a target language realizes a given Lamina primitive.
///
/// This is the vocabulary of the capability matrix — exactly five actions. All
/// but [`Capability::Forbid`] carry a target type name. `Widen`, `Alias`, and
/// `Wrap` all resolve to a target type but are distinguished for the benefit of
/// raising and diagnostics:
/// - `Alias` is a pure rename with no range change (`void` -> `()`).
/// - `Widen` maps to a broader type that represents every kernel value and more
///   (`i8` -> Python `int`); it may insert a conversion.
/// - `Wrap` supplies a named stand-in type when the target has no matching
///   primitive (`bytes` -> `Vec<u8>`); the value stands *in for* the primitive
///   rather than *being* it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Capability {
    /// The primitive maps to a target type of the same width, range, and
    /// meaning (e.g. `i32` -> `i32`).
    Identity(String),
    /// The primitive is a pure rename to another target type, with no range
    /// change (e.g. `void` -> `()`).
    Alias(String),
    /// The primitive is widened to a broader target type that can represent
    /// every kernel value and more (e.g. every integer -> `number`).
    Widen(String),
    /// The target has no matching primitive; a named stand-in type is supplied
    /// (e.g. `bytes` -> `Vec<u8>`, `str` -> `String`).
    Wrap(String),
    /// The primitive is not supported by this target and its use is an error.
    Forbid,
}

impl Capability {
    /// Returns the target type name this capability resolves to, or `None` if
    /// the primitive is forbidden.
    pub fn target_type(&self) -> Option<&str> {
        match self {
            Capability::Identity(name)
            | Capability::Alias(name)
            | Capability::Widen(name)
            | Capability::Wrap(name) => Some(name),
            Capability::Forbid => None,
        }
    }
}

/// The outcome selected by a slot or a `When` row: render a template, or
/// declare the construct forbidden in this target.
///
/// `Forbid` is a first-class, parse-time directive (the unquoted bareword
/// `forbid` in a `.mdl`), not a magic string compared after rendering. A slot
/// or row that resolves to `Forbid` makes emission fail with a
/// forbidden-construct error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Render this template.
    Render(Template),
    /// The construct is not expressible in this target.
    Forbid,
}

/// A `When` logic table: ordered predicate/outcome rows, evaluated top-to-bottom
/// with first-match-wins (`else` is the catch-all).
///
/// This is the sole branching primitive in a language definition. A branching
/// slot (`ret`, `vis`, `async`, …) is a `WhenTable`, selected against a
/// [`RenderContext`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhenTable {
    /// The rows, in evaluation order.
    pub rows: Vec<WhenRow>,
}

/// One row of a [`WhenTable`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhenRow {
    /// The predicate guarding this row.
    pub predicate: Predicate,
    /// The outcome selected when the predicate holds.
    pub outcome: Outcome,
}

impl WhenTable {
    /// Selects the first row whose predicate holds against `ctx`. Returns the
    /// row's outcome, or `None` if no row matched (a well-formed table ends in
    /// an `else` row, so `None` indicates a definition bug).
    pub fn select(&self, ctx: &RenderContext) -> Option<&Outcome> {
        self.rows
            .iter()
            .find(|row| ctx.eval(&row.predicate))
            .map(|row| &row.outcome)
    }
}

/// How a slot is resolved: a fixed outcome (no branching) or a `When` decision
/// table (branching). This is the uniform slot form — a `### <slot>` subsection
/// is one or the other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotDef {
    /// A single fixed outcome (a template, or `forbid`).
    Fixed(Outcome),
    /// A branching decision table.
    Table(WhenTable),
}

/// How a target renders a function.
///
/// `entry` is the single top-level template (authored as one `template` block
/// under `## Function`). Its `{slots}` — and any slots nested within slot
/// templates — resolve to same-named entries in `slots`, one heading level
/// deeper, unless they are engine-provided terminal slots (`name`, `params`,
/// `body`, `ret_type`). Placement of modifiers/visibility is fully data-driven
/// by where each slot appears in a template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionDef {
    /// The single entry template.
    pub entry: Template,
    /// Named slot definitions (each a fixed template or a decision table).
    pub slots: HashMap<String, SlotDef>,
}

/// How a target renders a non-function top-level item (`struct`, `enum`,
/// `typedef`, `const`, `use`).
///
/// Each such item kind gets its OWN `## <Item>` section in a language
/// definition — a sibling of `## Function` — with its own entry template and
/// `### <slot>` subsections. This mirrors [`FunctionDef`] exactly: `entry` is
/// the section's single `template` block and `slots` are its `### <slot>`
/// subsections.
///
/// Shared deep helpers (the `### expr` dispatch table used to render a
/// `const`'s value, the compound-type `### pointer`/`### fnptr` slots used to
/// render a field's type, …) live once under `## Function` and are reached by
/// the shared expression/type resolvers; an item section only declares the
/// slots unique to it (e.g. a struct's `### field` item slot, a `### vis`
/// spelling).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemDef {
    /// The single entry template for this item kind.
    pub entry: Template,
    /// Named slot definitions for this item kind (each a fixed template or a
    /// decision table).
    pub slots: HashMap<String, SlotDef>,
}

/// How a target realizes a kernel operator.
///
/// This is the MINIMAL viable operator-capability model (see the crate's spec
/// escalation): by default an operator emits its canonical Lamina spelling, so
/// a definition need only list the exceptions —
/// - a [`OperatorSpelling::Spell`] overrides the emitted text (e.g. a target
///   that spells floor-division as a function call), and
/// - [`OperatorSpelling::Forbid`] marks the operator inexpressible, making its
///   use a hard [`crate::error::EmitError::ForbiddenConstruct`].
///
/// A richer per-operator format (parenthesization / precedence, prefix vs infix
/// placement, call-style lowering) is deliberately deferred and escalated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperatorSpelling {
    /// Emit this exact text in place of the operator's canonical spelling.
    Spell(String),
    /// The operator is not expressible in this target; using it is an error.
    Forbid,
}

/// A complete language definition for one target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageDef {
    /// Human-readable target name, used in diagnostics (e.g. `"rust"`).
    pub name: String,
    /// The capability matrix: one entry per supported primitive.
    pub capabilities: HashMap<Primitive, Capability>,
    /// How this target renders a function.
    pub function: FunctionDef,
    /// How this target renders each non-function top-level item kind
    /// (`struct`, `enum`, `typedef`, `const`, `use`), keyed by [`ItemKind`].
    /// Each entry is parsed from that kind's own `## <Item>` section. Functions
    /// are rendered via [`LanguageDef::function`] rather than this map.
    pub items: HashMap<ItemKind, ItemDef>,
    /// Per-operator spelling overrides and forbids, keyed by the operator's
    /// stable machine name (see [`crate::ast::UnaryOp::name`] /
    /// [`crate::ast::BinaryOp::name`]). An operator absent from this map emits
    /// its canonical spelling — this is the common case, so most definitions
    /// leave the map empty or list only their exceptions.
    pub operators: HashMap<String, OperatorSpelling>,
}

impl LanguageDef {
    /// Looks up how this target realizes `primitive`.
    ///
    /// Returns `None` if the definition has no entry for the primitive at all
    /// (treated as unsupported, same as [`Capability::Forbid`]).
    pub fn capability(&self, primitive: Primitive) -> Option<&Capability> {
        self.capabilities.get(&primitive)
    }

    /// Looks up any per-operator spelling override or forbid for the operator
    /// with machine name `op_name`. Returns `None` when the target uses the
    /// operator's canonical spelling (the default).
    pub fn operator(&self, op_name: &str) -> Option<&OperatorSpelling> {
        self.operators.get(op_name)
    }

    /// Looks up how this target renders the non-function item kind `kind`.
    /// Returns `None` when the definition has no `## <Item>` section for that
    /// kind (using such an item is then an emit-time error).
    pub fn item_def(&self, kind: ItemKind) -> Option<&ItemDef> {
        self.items.get(&kind)
    }
}
