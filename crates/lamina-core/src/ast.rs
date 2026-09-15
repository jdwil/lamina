//! The Lamina abstract syntax tree (AST).
//!
//! This is the in-memory representation of parsed Lamina IR. It is deliberately
//! minimal for the first end-to-end slice: a file containing functions, where a
//! function returns a single integer literal. The AST is *structure-preserving*
//! and *target-agnostic* — it carries no knowledge of any target language. All
//! target-specific decisions live in a [`crate::lang`] language definition.

/// A parsed Lamina source unit (the contents of one `lamina` code block).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    /// The top-level function definitions in this unit.
    pub functions: Vec<Function>,
}

/// A function definition.
///
/// The minimal slice supports a name, no parameters, a single return type, and
/// a body. Parameters and richer bodies are added in later slices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    /// The function's identifier.
    pub name: String,
    /// The declared return type.
    pub return_type: Type,
    /// The statements forming the function body.
    pub body: Vec<Statement>,
}

/// A Lamina type reference.
///
/// For the minimal slice this is only a kernel primitive. Named/user types are
/// introduced in a later slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    /// One of the Lamina kernel primitives (see [`Primitive`]).
    Primitive(Primitive),
}

/// The Lamina kernel primitive types.
///
/// This is the frozen set the engine understands. A language definition's
/// capability matrix decides how each primitive is realized (or forbidden) in a
/// given target. Only the primitives needed by the first slice are enumerated
/// here; the full set is added incrementally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Primitive {
    /// Signed 32-bit integer.
    I32,
}

impl Primitive {
    /// Returns the canonical Lamina spelling of this primitive.
    pub fn as_str(self) -> &'static str {
        match self {
            Primitive::I32 => "i32",
        }
    }

    /// Resolves a Lamina primitive from its canonical spelling.
    ///
    /// Returns `None` if `name` is not a known primitive. Only the primitives
    /// supported by the current slice are recognized; the set grows over time.
    pub fn from_name(name: &str) -> Option<Primitive> {
        match name {
            "i32" => Some(Primitive::I32),
            _ => None,
        }
    }
}

/// A statement within a function body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Statement {
    /// A `return <expr>;` statement.
    Return(Expr),
}

/// An expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    /// An integer literal, preserved as its textual form to avoid premature
    /// width/precision decisions (those belong to the target).
    IntLiteral(String),
}
