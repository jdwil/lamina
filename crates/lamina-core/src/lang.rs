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
use crate::error::LangDocError;
use crate::predicate::{Predicate, RenderContext};
use crate::render::Template;

/// The `.mdl` **format** version this engine understands, as a `major.minor.patch`
/// semver string. A language definition declares (in its `lang-meta` header) the
/// MINIMUM format version it requires via `lamina-format:`; the engine refuses to
/// load a definition whose required version is NEWER than this constant, and loads
/// any definition requiring an equal or older version (the engine is
/// backward-compatible).
///
/// It starts at `0.0.0` (alpha): the format is not yet stabilized. Bump it when a
/// backward-incompatible or additive change to the `.mdl` format is made, so older
/// engines correctly refuse definitions that rely on the newer format.
pub const LAMINA_FORMAT_VERSION: &str = "0.0.0";

/// A parsed 3-integer semantic version (`major.minor.patch`).
///
/// This is a deliberately tiny, dependency-free comparator: the engine only needs
/// to answer "is version A newer than version B?" for the format-version gate, so a
/// full semver crate (with pre-release/build metadata, ranges, etc.) is unwarranted.
/// Ordering is the natural lexicographic order over the `(major, minor, patch)`
/// tuple, which is exactly semver's precedence for release versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    /// The major component.
    pub major: u64,
    /// The minor component.
    pub minor: u64,
    /// The patch component.
    pub patch: u64,
}

impl Version {
    /// Parses a `major.minor.patch` string into a [`Version`].
    ///
    /// The string MUST have exactly three dot-separated non-negative integer
    /// components (no pre-release/build suffixes, no missing or extra parts). This
    /// is intentionally strict: a malformed version is a load-time error, never a
    /// silently-tolerated default.
    ///
    /// # Errors
    ///
    /// Returns [`LangDocError::MalformedFormatVersion`] if the string does not have
    /// exactly three components or any component is not a non-negative integer.
    pub fn parse(s: &str) -> Result<Version, LangDocError> {
        let bad = || LangDocError::MalformedFormatVersion {
            value: s.to_string(),
        };
        let mut parts = s.split('.');
        let major = parts.next().ok_or_else(bad)?;
        let minor = parts.next().ok_or_else(bad)?;
        let patch = parts.next().ok_or_else(bad)?;
        if parts.next().is_some() {
            return Err(bad());
        }
        // Reject empty components and any non-digit (e.g. `1.2.x`, `1..3`, `-1.0.0`).
        let parse_component = |c: &str| -> Result<u64, LangDocError> {
            if c.is_empty() || !c.bytes().all(|b| b.is_ascii_digit()) {
                return Err(bad());
            }
            c.parse::<u64>().map_err(|_| bad())
        };
        Ok(Version {
            major: parse_component(major)?,
            minor: parse_component(minor)?,
            patch: parse_component(patch)?,
        })
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

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
    /// The optional pass/region annotations scoping this row (see
    /// [`RuleScope`]). Both are `None` in a definition that declares no
    /// `## Passes` section, so an unannotated row behaves exactly as before
    /// (active in the single implicit pass, emitting inline).
    pub scope: RuleScope,
}

/// The optional pass/region annotations a slot subsection or `When`-table row
/// may carry. Both fields default to `None`, which is the legacy behavior: the
/// rule is active in every pass (the single implicit pass when no `## Passes`
/// section is declared) and emits into the default/inline output.
///
/// The engine attaches NO meaning to a pass or region name — they are opaque
/// author-chosen identifiers. `pass` scopes *when* a rule is active (only during
/// the named pass); `region` scopes *where* its rendered output goes (into the
/// named output buffer instead of inline). See the multi-pass driver in
/// [`crate::emitter::emit`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuleScope {
    /// The pass this rule is active in, or `None` for "every pass".
    pub pass: Option<String>,
    /// The output region this rule's rendered text is routed to, or `None` for
    /// the default/inline output.
    pub region: Option<String>,
}

impl RuleScope {
    /// An unannotated scope: active in all passes, emitting inline. Equivalent
    /// to `RuleScope::default()`.
    pub fn none() -> Self {
        RuleScope::default()
    }

    /// Whether this rule is active during `current_pass`. A rule with no `pass`
    /// annotation is active in every pass; a rule annotated `pass: X` is active
    /// only when the current pass is `X`. In legacy single-pass mode
    /// (`current_pass` is `None`), only unannotated rules are active — a
    /// definition that declares no passes must not carry `pass:` annotations
    /// (the loader rejects an unknown pass name), so this is vacuously true
    /// there.
    pub fn active_in(&self, current_pass: Option<&str>) -> bool {
        match &self.pass {
            None => true,
            Some(p) => current_pass == Some(p.as_str()),
        }
    }
}

impl WhenTable {
    /// Selects the first row whose predicate holds against `ctx` **and** whose
    /// pass annotation is active in `current_pass`. Returns the matched row (so
    /// the caller can read its [`RuleScope`] for region routing), or `None` if
    /// no row matched.
    ///
    /// A row annotated `pass: X` is only considered during pass `X`; an
    /// unannotated row is considered in every pass. In legacy single-pass mode
    /// (`current_pass` is `None`) every row is unannotated (the loader forbids
    /// `pass:` without a `## Passes` section), so selection is unchanged and a
    /// well-formed table still ends in an `else` row.
    pub fn select(&self, ctx: &RenderContext, current_pass: Option<&str>) -> Option<&WhenRow> {
        self.rows
            .iter()
            .find(|row| row.scope.active_in(current_pass) && ctx.eval(&row.predicate))
    }
}

/// How a slot is resolved: a fixed outcome (no branching) or a `When` decision
/// table (branching). This is the uniform slot form — a `### <slot>` subsection
/// is one or the other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotDef {
    /// A single fixed outcome (a template, or `forbid`), with its optional
    /// pass/region annotation.
    Fixed {
        /// The fixed outcome to render.
        outcome: Outcome,
        /// The pass/region annotation scoping this fixed slot.
        scope: RuleScope,
    },
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

/// The multi-pass + output-region plan a definition declares in its
/// `## Passes` section (a ```` ```lang-passes ```` block).
///
/// This is the entirety of what the engine knows about passes and regions: an
/// ordered list of opaque pass names, an ordered list of opaque region names,
/// and the layout (the order regions concatenate into the final output). The
/// engine attaches NO semantics — it drives the passes in order and assembles
/// the region buffers per `layout`; all meaning lives in the definition's
/// `pass:` / `region:` rule annotations.
///
/// An **empty** plan (`passes` empty) is the legacy case: no `## Passes`
/// section, a single implicit pass, inline output, byte-identical to before
/// this mechanism existed. [`PassPlan::is_multipass`] distinguishes the two.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PassPlan {
    /// The ordered, author-named passes. The engine renders the whole unit once
    /// per pass in this order. Empty means "no `## Passes` section" (single
    /// implicit pass).
    pub passes: Vec<String>,
    /// The author-named output regions. A conventional `body` region receives
    /// inline/unrouted emission; other regions receive rule output annotated
    /// `region: <name>`.
    pub regions: Vec<String>,
    /// The concatenation order of regions in the final assembled output. Every
    /// name here MUST be a declared region; `body` may appear to position the
    /// inline output.
    pub layout: Vec<String>,
}

impl PassPlan {
    /// Whether this definition declares any passes (i.e. carries a `## Passes`
    /// section). When `false`, the emitter uses the legacy single-pass,
    /// inline-output path — byte-identical to pre-multipass output.
    pub fn is_multipass(&self) -> bool {
        !self.passes.is_empty()
    }
}

/// A complete language definition for one target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageDef {
    /// Human-readable target name, used in diagnostics (e.g. `"rust"`).
    pub name: String,
    /// The target language this definition emits (from the `lang-meta` header's
    /// `target:` field). Usually the same as [`LanguageDef::name`], but declared
    /// explicitly so the engine carries an authoritative language identity for
    /// future registry/resolution. Stored verbatim; the engine never parses it.
    pub target: String,
    /// The target-language version band this definition emits for (from the
    /// `lang-meta` header's `target-version:` field) — e.g. Rust edition `2021`,
    /// `>=3.0` for Python, `>=5.0` for TypeScript.
    ///
    /// This is an **opaque** string: the engine stores it verbatim and NEVER
    /// parses, compares, or branches on it. Version-specific behavior lives in
    /// SEPARATE definitions (a conservative `python` def and a permissive
    /// `python3.13` def are two distinct files), not in engine conditionals or
    /// `When`-language predicates. A future registry/resolution layer may filter
    /// candidate definitions by this band; the engine itself performs no version
    /// logic on it.
    pub target_version: String,
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
    /// The multi-pass + output-region plan (from the optional `## Passes`
    /// section). An empty plan means the definition declares no passes and the
    /// emitter uses the legacy single-pass, inline-output path (byte-identical
    /// to pre-multipass output).
    pub passes: PassPlan,
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

    /// Returns the opaque target-language version band this definition declares
    /// (the `lang-meta` header's `target-version:`). The engine never acts on
    /// this value; it is carried verbatim for a future registry/resolution layer.
    pub fn target_version(&self) -> &str {
        &self.target_version
    }
}
