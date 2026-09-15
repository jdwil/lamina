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

use crate::ast::Primitive;

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

/// Surface-syntax parameters for emitting a function in a target language.
///
/// Kept as data (not code) so that adding or tweaking a target does not require
/// engine changes. This is minimal for the first slice and will grow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionSyntax {
    /// Keyword introducing a function definition (e.g. `fn`, `function`).
    pub keyword: String,
    /// Rendered between the parameter list and the return type (e.g. `" -> "`
    /// for Rust, `": "` for TypeScript).
    pub return_type_sep: String,
    /// Whether the return type annotation is emitted at all. Some targets
    /// (dynamically typed) may omit it.
    pub emit_return_type: bool,
}

/// A complete language definition for one target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageDef {
    /// Human-readable target name, used in diagnostics (e.g. `"rust"`).
    pub name: String,
    /// The capability matrix: one entry per supported primitive.
    pub capabilities: HashMap<Primitive, Capability>,
    /// Function surface-syntax parameters.
    pub function_syntax: FunctionSyntax,
}

impl LanguageDef {
    /// Looks up how this target realizes `primitive`.
    ///
    /// Returns `None` if the definition has no entry for the primitive at all
    /// (treated as unsupported, same as [`Capability::Forbid`]).
    pub fn capability(&self, primitive: Primitive) -> Option<&Capability> {
        self.capabilities.get(&primitive)
    }
}
