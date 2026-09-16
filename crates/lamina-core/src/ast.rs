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
    /// The top-level items in this unit, in source order.
    ///
    /// A file interleaves declarations — functions, structs, enums, type
    /// aliases, constants, and imports — so a single ordered `Vec<Item>`
    /// preserves their relative order rather than bucketing each kind into its
    /// own list.
    pub items: Vec<Item>,
}

/// A top-level item (declaration) in a [`File`].
///
/// This is the complete kernel item set: a callable [`Function`], a `struct`,
/// an `enum`, a type alias (`typedef`), an item-level `const`, and a `use`
/// import. Each item kind is rendered by dispatching on its [`ItemKind`] (the
/// `item is <kind>` fact) to the target's matching `## <Item>` section.
///
/// Two areas are deliberately deferred and documented rather than dropped:
/// - **Enum payloads / associated data.** [`Variant`] carries only a name;
///   targets differ wildly on how they attach data to a variant (Rust tuple/
///   struct variants, C's plain integer enumerators, TypeScript's union types),
///   so a plain C-like enum of names is the safe first pass.
/// - **Structured `use` lists.** [`Item::Use`] carries a single `path` string;
///   grouped/aliased import lists (`use a::{b, c as d}`) are a later
///   refinement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// A function definition (see [`Function`]).
    Function(Function),
    /// A `struct`: a named aggregate of typed fields.
    Struct {
        /// The struct's identifier.
        name: String,
        /// The struct's visibility.
        visibility: Visibility,
        /// The fields, in declaration order (possibly empty).
        fields: Vec<Field>,
    },
    /// An `enum`: a named set of variants. Variants carry only a name for now
    /// (payloads are deferred — see [`Item`]).
    Enum {
        /// The enum's identifier.
        name: String,
        /// The enum's visibility.
        visibility: Visibility,
        /// The variants, in declaration order (possibly empty).
        variants: Vec<Variant>,
    },
    /// A type alias (`typedef`): a name bound to a target [`Type`].
    TypeDef {
        /// The alias's identifier.
        name: String,
        /// The aliased type.
        target: Type,
    },
    /// An item-level constant (`const name: ty = value;`).
    Const {
        /// The constant's identifier.
        name: String,
        /// The constant's declared type.
        ty: Type,
        /// The constant's value expression.
        value: Expr,
        /// The constant's visibility.
        visibility: Visibility,
    },
    /// An import (`use path;`). The `path` is a single string for now
    /// (structured import lists are deferred — see [`Item`]).
    Use {
        /// The imported path, verbatim.
        path: String,
    },
}

impl Item {
    /// The dispatch kind of this item, used to select the target's matching
    /// `## <Item>` section (via the `item is <kind>` fact).
    pub fn kind(&self) -> ItemKind {
        match self {
            Item::Function(_) => ItemKind::Function,
            Item::Struct { .. } => ItemKind::Struct,
            Item::Enum { .. } => ItemKind::Enum,
            Item::TypeDef { .. } => ItemKind::TypeDef,
            Item::Const { .. } => ItemKind::Const,
            Item::Use { .. } => ItemKind::Use,
        }
    }
}

/// A `struct` field: a name, a type, and a visibility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// The field's identifier.
    pub name: String,
    /// The field's declared type.
    pub ty: Type,
    /// The field's visibility.
    pub visibility: Visibility,
}

/// An `enum` variant. Carries only a name for now; associated data / payloads
/// are deferred (see [`Item`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    /// The variant's identifier.
    pub name: String,
}

/// The dispatch kind of an [`Item`], answering the `item is <kind>` fact.
///
/// This is a closed set that mirrors the [`Item`] variants one-to-one, so the
/// engine can dispatch each item to the target's matching `## <Item>` section,
/// and a `When` table can branch on the item kind if needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ItemKind {
    /// A function.
    Function,
    /// A `struct`.
    Struct,
    /// An `enum`.
    Enum,
    /// A type alias.
    TypeDef,
    /// An item-level constant.
    Const,
    /// A `use` import.
    Use,
}

impl ItemKind {
    /// The canonical `item is <kind>` value spelling for this kind. This is also
    /// the spelling of the item's `## <Item>` section heading (title-cased):
    /// `function` -> `## Function`, `typedef` -> `## TypeDef`, etc.
    pub fn as_str(self) -> &'static str {
        match self {
            ItemKind::Function => "function",
            ItemKind::Struct => "struct",
            ItemKind::Enum => "enum",
            ItemKind::TypeDef => "typedef",
            ItemKind::Const => "const",
            ItemKind::Use => "use",
        }
    }

    /// The `## <Item>` section heading spelling for this kind (e.g. `Function`,
    /// `TypeDef`). Keeps the heading vocabulary in one place, shared by the
    /// language-definition parser.
    pub fn heading(self) -> &'static str {
        match self {
            ItemKind::Function => "Function",
            ItemKind::Struct => "Struct",
            ItemKind::Enum => "Enum",
            ItemKind::TypeDef => "TypeDef",
            ItemKind::Const => "Const",
            ItemKind::Use => "Use",
        }
    }

    /// The [`SlotScope`] the item's entry-template slots resolve in.
    pub fn scope(self) -> SlotScope {
        match self {
            ItemKind::Function => SlotScope::Function,
            ItemKind::Struct => SlotScope::Struct,
            ItemKind::Enum => SlotScope::Enum,
            ItemKind::TypeDef => SlotScope::TypeDef,
            ItemKind::Const => SlotScope::Const,
            ItemKind::Use => SlotScope::Use,
        }
    }

    /// Every item kind, in canonical order. Keeps the closed `item` vocabulary
    /// in one place, shared by the predicate registry and the language-def
    /// parser (which parses one `## <Item>` section per kind).
    pub fn all() -> [ItemKind; 6] {
        [
            ItemKind::Function,
            ItemKind::Struct,
            ItemKind::Enum,
            ItemKind::TypeDef,
            ItemKind::Const,
            ItemKind::Use,
        ]
    }
}

/// A function definition.
///
/// The minimal slice supports a name, no parameters, a single return type, and
/// a body. Parameters and richer bodies are added in later slices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    /// The function's identifier.
    pub name: String,
    /// Visibility of the function (defaults to [`Visibility::Private`] when the
    /// source omits it).
    pub visibility: Visibility,
    /// The on/off modifiers applied to this function, in source order.
    ///
    /// The kernel reserves the *superset* of callable modifiers; a language
    /// definition decides how each is spelled or whether it is forbidden. A
    /// modifier is semantic metadata first and emitted text second — it remains
    /// on the node even for targets whose spelling is empty.
    pub modifiers: Vec<Modifier>,
    /// The function's parameters, in order (possibly empty). The concrete
    /// source syntax for declaring parameters is not yet defined, so the parser
    /// currently produces an empty list; the AST field and rendering path exist
    /// so the collection-slot machinery is in place.
    pub params: Vec<Param>,
    /// The declared return type.
    pub return_type: Type,
    /// The statements forming the function body (possibly empty).
    pub body: Vec<Statement>,
}

/// A function parameter: a name and a type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    /// The parameter's identifier.
    pub name: String,
    /// The parameter's declared type.
    pub ty: Type,
}

impl Function {
    /// Returns `true` if this function carries the given modifier.
    pub fn has_modifier(&self, modifier: Modifier) -> bool {
        self.modifiers.contains(&modifier)
    }
}

/// Visibility of a callable.
///
/// Three kernel levels. A language definition maps each level to its target
/// spelling or forbids it. For free functions (kernel v0, no inheritance)
/// `Protected` is the middle tier — more than private, less than public — and
/// maps to a target's module/package visibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Visibility {
    /// Visible everywhere.
    Public,
    /// Visible within the module/package (the middle tier).
    Protected,
    /// Visible only within the defining scope. The default when source omits a
    /// visibility.
    Private,
}

impl Visibility {
    /// The canonical Lamina spelling of this visibility level.
    pub fn as_str(self) -> &'static str {
        match self {
            Visibility::Public => "public",
            Visibility::Protected => "protected",
            Visibility::Private => "private",
        }
    }

    /// Resolves a visibility level from its canonical spelling.
    pub fn from_name(name: &str) -> Option<Visibility> {
        match name {
            "public" => Some(Visibility::Public),
            "protected" => Some(Visibility::Protected),
            "private" => Some(Visibility::Private),
            _ => None,
        }
    }
}

/// An on/off callable modifier.
///
/// This is the kernel's reserved superset: every modifier that can apply to a
/// callable in *any* target language. The engine treats them uniformly — a
/// language definition supplies each one's spelling or forbids it. Behavior
/// (how `async` actually runs, how `throws` propagates) lives in layers or the
/// target runtime, never the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Modifier {
    /// Compile-time evaluable (e.g. Rust `const fn`).
    Const,
    /// Suspendable / awaitable (e.g. Rust/TS `async`, Kotlin `suspend`).
    Async,
    /// Opts out of safety checks (e.g. Rust `unsafe`).
    Unsafe,
    /// Declares error propagation (e.g. Swift `throws`).
    Throws,
    /// Foreign linkage / ABI (e.g. Rust `extern`).
    Extern,
    /// Inlining directive (e.g. Kotlin `inline fun`).
    Inline,
    /// Produces a sequence lazily (e.g. Python/JS generators). Often emits no
    /// declaration text but is retained as metadata.
    Generator,
}

impl Modifier {
    /// The canonical Lamina spelling of this modifier keyword.
    pub fn as_str(self) -> &'static str {
        match self {
            Modifier::Const => "const",
            Modifier::Async => "async",
            Modifier::Unsafe => "unsafe",
            Modifier::Throws => "throws",
            Modifier::Extern => "extern",
            Modifier::Inline => "inline",
            Modifier::Generator => "generator",
        }
    }

    /// Resolves a modifier from its canonical keyword spelling.
    pub fn from_name(name: &str) -> Option<Modifier> {
        match name {
            "const" => Some(Modifier::Const),
            "async" => Some(Modifier::Async),
            "unsafe" => Some(Modifier::Unsafe),
            "throws" => Some(Modifier::Throws),
            "extern" => Some(Modifier::Extern),
            "inline" => Some(Modifier::Inline),
            "generator" => Some(Modifier::Generator),
            _ => None,
        }
    }

    /// All modifiers in the reserved kernel superset, in canonical order.
    pub fn all() -> [Modifier; 7] {
        [
            Modifier::Const,
            Modifier::Async,
            Modifier::Unsafe,
            Modifier::Throws,
            Modifier::Extern,
            Modifier::Inline,
            Modifier::Generator,
        ]
    }
}

/// A Lamina type reference.
///
/// A type is either a kernel [`Primitive`], a reference to a user-defined type
/// by [`name`](Type::Named), or one of the compound forms (a
/// [`Pointer`](Type::Pointer) or a [`FnPtr`](Type::FnPtr)). Compound forms are
/// gated by the corresponding capability (`ptr`, `fnptr`): a target that
/// `forbid`s the primitive cannot express the compound type either.
///
/// Arrays (`[T; N]`) are intentionally *deferred* — see spec 01-types.md. They
/// may be better modelled as a layer/collection type and are omitted from this
/// pass to keep the kernel minimal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    /// One of the Lamina kernel primitives (see [`Primitive`]).
    Primitive(Primitive),
    /// A reference to a user-defined type (a `struct`/`enum`/`typedef` name).
    ///
    /// The name is rendered verbatim for now; a language definition may remap
    /// it in a later slice.
    Named(String),
    /// A pointer to another type (kernel `ptr`). Rendering is target-specific
    /// (e.g. Rust `*const T`) and supplied by the language definition's
    /// `### pointer` slot via the `pointee` sub-slot. Gated by the `ptr`
    /// capability.
    Pointer(Box<Type>),
    /// A function pointer (kernel `fnptr`). Rendering is target-specific and
    /// supplied by the language definition's `### fnptr` slot via the `params`
    /// (a sequence of parameter types) and `ret` sub-slots. Gated by the
    /// `fnptr` capability.
    FnPtr {
        /// The parameter types, in order (possibly empty).
        params: Vec<Type>,
        /// The return type.
        ret: Box<Type>,
    },
}

/// The Lamina kernel primitive types.
///
/// This is the **frozen** set the engine understands — the complete kernel
/// primitive vocabulary. A language definition's capability matrix decides how
/// each primitive is realized (or forbidden) in a given target, and the matrix
/// MUST cover every one of these (completeness is enforced at load time).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Primitive {
    /// Signed 8-bit integer.
    I8,
    /// Signed 16-bit integer.
    I16,
    /// Signed 32-bit integer.
    I32,
    /// Signed 64-bit integer.
    I64,
    /// Signed 128-bit integer.
    I128,
    /// Unsigned 8-bit integer.
    U8,
    /// Unsigned 16-bit integer.
    U16,
    /// Unsigned 32-bit integer.
    U32,
    /// Unsigned 64-bit integer.
    U64,
    /// Unsigned 128-bit integer.
    U128,
    /// Pointer-sized signed integer.
    Isize,
    /// Pointer-sized unsigned integer.
    Usize,
    /// 16-bit (IEEE half) float.
    F16,
    /// 16-bit brain float.
    Bf16,
    /// 32-bit float.
    F32,
    /// 64-bit float.
    F64,
    /// 128-bit float.
    F128,
    /// Boolean.
    Bool,
    /// The unit / no-value type.
    Void,
    /// The uninhabited / never-returns type.
    Never,
    /// A single octet (byte), distinct from `u8` in intent (octet meaning).
    Byte,
    /// A sequence of bytes.
    Bytes,
    /// A single Unicode scalar value.
    Char,
    /// A string of text.
    Str,
    /// A raw pointer.
    Ptr,
    /// A function pointer.
    Fnptr,
}

impl Primitive {
    /// Returns the canonical Lamina spelling of this primitive.
    pub fn as_str(self) -> &'static str {
        match self {
            Primitive::I8 => "i8",
            Primitive::I16 => "i16",
            Primitive::I32 => "i32",
            Primitive::I64 => "i64",
            Primitive::I128 => "i128",
            Primitive::U8 => "u8",
            Primitive::U16 => "u16",
            Primitive::U32 => "u32",
            Primitive::U64 => "u64",
            Primitive::U128 => "u128",
            Primitive::Isize => "isize",
            Primitive::Usize => "usize",
            Primitive::F16 => "f16",
            Primitive::Bf16 => "bf16",
            Primitive::F32 => "f32",
            Primitive::F64 => "f64",
            Primitive::F128 => "f128",
            Primitive::Bool => "bool",
            Primitive::Void => "void",
            Primitive::Never => "never",
            Primitive::Byte => "byte",
            Primitive::Bytes => "bytes",
            Primitive::Char => "char",
            Primitive::Str => "str",
            Primitive::Ptr => "ptr",
            Primitive::Fnptr => "fnptr",
        }
    }

    /// Resolves a Lamina primitive from its canonical spelling. Returns `None`
    /// if `name` is not a kernel primitive.
    pub fn from_name(name: &str) -> Option<Primitive> {
        Primitive::all()
            .into_iter()
            .find(|p| p.as_str() == name)
    }

    /// The complete frozen kernel primitive set, in canonical order.
    pub fn all() -> [Primitive; 26] {
        [
            Primitive::I8,
            Primitive::I16,
            Primitive::I32,
            Primitive::I64,
            Primitive::I128,
            Primitive::U8,
            Primitive::U16,
            Primitive::U32,
            Primitive::U64,
            Primitive::U128,
            Primitive::Isize,
            Primitive::Usize,
            Primitive::F16,
            Primitive::Bf16,
            Primitive::F32,
            Primitive::F64,
            Primitive::F128,
            Primitive::Bool,
            Primitive::Void,
            Primitive::Never,
            Primitive::Byte,
            Primitive::Bytes,
            Primitive::Char,
            Primitive::Str,
            Primitive::Ptr,
            Primitive::Fnptr,
        ]
    }
}

/// A statement within a function body or a nested block.
///
/// This is the complete kernel statement set. Control flow is *statement*-based
/// (not expression-based): a language definition whose target treats `if` or
/// `switch` as an expression renders that in its own `### statement` row — the
/// kernel stays the lowest common denominator that lowers cleanly to C, Rust,
/// TypeScript, and Python alike.
///
/// Each statement kind is rendered by the target's `### statement` `When`-table,
/// which dispatches on the [`StatementKind`] (the `stmt is <kind>` fact) and
/// fills that row's scoped sub-slots (`cond`, `then`, `else`, `body`, `name`,
/// `let_type`, `value`, `binding`, `iterable`, `scrutinee`, `cases`, …).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Statement {
    /// A scoped sequence of statements (`{ … }`). First-class; used as the body
    /// of nested scopes. (A function's own body is a `Vec<Statement>` with block
    /// semantics; `Block` is for *nested* scopes.)
    Block(Vec<Statement>),
    /// A local binding (`let name: ty = value;`). Both `ty` and `value` are
    /// optional: `value` is absent for declared-but-unassigned locals (needed
    /// for the C hoisting case), and `ty` is absent for inferred targets.
    Let {
        /// The bound identifier.
        name: String,
        /// The declared type, or `None` when the target infers it.
        ty: Option<Type>,
        /// The initializer expression, or `None` when the local is declared
        /// unassigned.
        value: Option<Expr>,
    },
    /// A `return;` or `return <expr>;` statement. The value is optional: a bare
    /// `return;` is valid, and a `void` function returns nothing.
    Return(Option<Expr>),
    /// An `if` statement with an optional `else`. The `else_block` may be
    /// another `If` (an else-if chain) or a `Block` — both are statements, so it
    /// is stored as a boxed [`Statement`].
    If {
        /// The condition expression.
        cond: Expr,
        /// The consequent statements.
        then_block: Vec<Statement>,
        /// The optional `else` branch: another `If` (else-if) or a `Block`.
        else_block: Option<Box<Statement>>,
    },
    /// A `while <cond> { … }` loop.
    While {
        /// The loop condition.
        cond: Expr,
        /// The loop body.
        body: Vec<Statement>,
    },
    /// A C-style counted loop (`for (init; cond; step) { … }`). Each of `init`,
    /// `cond`, and `step` is optional.
    For {
        /// The optional initializer statement (e.g. a `let`).
        init: Option<Box<Statement>>,
        /// The optional continuation condition.
        cond: Option<Expr>,
        /// The optional step statement (e.g. an expression-statement).
        step: Option<Box<Statement>>,
        /// The loop body.
        body: Vec<Statement>,
    },
    /// An iterator loop (`foreach binding in iterable { … }`). (`foreach` is a
    /// distinct kernel keyword from the counted `for`.)
    ForEach {
        /// The element binding name.
        binding: String,
        /// The iterable expression.
        iterable: Expr,
        /// The loop body.
        body: Vec<Statement>,
    },
    /// A `switch` over a scrutinee with zero or more cases and an optional
    /// default branch.
    Switch {
        /// The value being switched on.
        scrutinee: Expr,
        /// The cases, in order.
        cases: Vec<SwitchCase>,
        /// The optional default branch's statements.
        default: Option<Vec<Statement>>,
    },
    /// A `break;` statement (labels deferred).
    Break,
    /// A `continue;` statement (labels deferred).
    Continue,
    /// An expression-statement (e.g. a bare function call `f();`).
    Expr(Expr),
}

/// One `case` of a [`Statement::Switch`]: a matched value and its body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchCase {
    /// The value this case matches against the scrutinee.
    pub value: Expr,
    /// The statements run when the case matches.
    pub body: Vec<Statement>,
}

impl Statement {
    /// The dispatch kind of this statement, used to select the target's
    /// `### statement` `When`-table row (via the `stmt is <kind>` fact).
    pub fn kind(&self) -> StatementKind {
        match self {
            Statement::Block(_) => StatementKind::Block,
            Statement::Let { .. } => StatementKind::Let,
            Statement::Return(_) => StatementKind::Return,
            Statement::If { .. } => StatementKind::If,
            Statement::While { .. } => StatementKind::While,
            Statement::For { .. } => StatementKind::For,
            Statement::ForEach { .. } => StatementKind::ForEach,
            Statement::Switch { .. } => StatementKind::Switch,
            Statement::Break => StatementKind::Break,
            Statement::Continue => StatementKind::Continue,
            Statement::Expr(_) => StatementKind::Expr,
        }
    }
}

/// The dispatch kind of a [`Statement`], answering the `stmt is <kind>` fact.
///
/// This is a closed set that mirrors the [`Statement`] variants one-to-one, so
/// a language definition can render each kind through a single `### statement`
/// `When`-table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatementKind {
    /// A nested block.
    Block,
    /// A local binding.
    Let,
    /// A return.
    Return,
    /// An `if`/`else`.
    If,
    /// A `while` loop.
    While,
    /// A counted `for` loop.
    For,
    /// A `foreach` iterator loop.
    ForEach,
    /// A `switch`.
    Switch,
    /// A `break`.
    Break,
    /// A `continue`.
    Continue,
    /// An expression-statement.
    Expr,
}

impl StatementKind {
    /// The canonical `stmt is <kind>` value spelling for this kind.
    pub fn as_str(self) -> &'static str {
        match self {
            StatementKind::Block => "block",
            StatementKind::Let => "let",
            StatementKind::Return => "return",
            StatementKind::If => "if",
            StatementKind::While => "while",
            StatementKind::For => "for",
            StatementKind::ForEach => "foreach",
            StatementKind::Switch => "switch",
            StatementKind::Break => "break",
            StatementKind::Continue => "continue",
            StatementKind::Expr => "expr",
        }
    }

    /// Every statement kind, in canonical order. Keeps the closed `stmt`
    /// vocabulary in one place, shared by the predicate registry.
    pub fn all() -> [StatementKind; 11] {
        [
            StatementKind::Block,
            StatementKind::Let,
            StatementKind::Return,
            StatementKind::If,
            StatementKind::While,
            StatementKind::For,
            StatementKind::ForEach,
            StatementKind::Switch,
            StatementKind::Break,
            StatementKind::Continue,
            StatementKind::Expr,
        ]
    }
}

/// An expression.
///
/// The complete kernel expression tree: literals, references, member/index
/// access, calls, and unary/binary operators. It is a *structural* tree — an
/// expression's grouping is encoded by nesting, not by stored precedence, so
/// precedence is a purely parsing concern (deferred with the source grammar).
///
/// Literals that carry target-sensitive width, precision, or escaping (int,
/// float, string, char) are preserved *textually*: the engine never commits to
/// a numeric width or a quoting/escaping scheme — those are the language
/// definition's decisions, made in the relevant slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    /// An integer literal, preserved as its textual form to avoid premature
    /// width/precision decisions (those belong to the target).
    IntLiteral(String),
    /// A floating-point literal, preserved textually (precision is a target
    /// concern).
    FloatLiteral(String),
    /// A boolean literal (`true` / `false`).
    BoolLiteral(bool),
    /// A string literal. The stored value is the *contents* (already unescaped);
    /// the language definition applies the target's quoting and escaping via a
    /// slot.
    StringLiteral(String),
    /// A character literal. The stored value is the single character's contents
    /// (already unescaped); the language definition applies target quoting.
    CharLiteral(String),
    /// The kernel `null` literal.
    ///
    /// `null` **follows `ptr`**: a target whose capability matrix forbids `ptr`
    /// must also forbid `null`. The emitter surfaces this by treating a `null`
    /// literal as forbidden whenever the target forbids the `ptr` primitive.
    NullLiteral,
    /// A variable / identifier reference.
    Ref(String),
    /// Field access (`obj.field`).
    Field {
        /// The object being accessed.
        obj: Box<Expr>,
        /// The field name.
        field: String,
    },
    /// Indexing (`obj[index]`).
    Index {
        /// The object being indexed.
        obj: Box<Expr>,
        /// The index expression.
        index: Box<Expr>,
    },
    /// Function application (`callee(args...)`).
    Call {
        /// The expression being called.
        callee: Box<Expr>,
        /// The argument expressions, in order (possibly empty).
        args: Vec<Expr>,
    },
    /// A unary operator applied to a single operand.
    Unary {
        /// The operator.
        op: UnaryOp,
        /// The operand.
        operand: Box<Expr>,
    },
    /// A binary operator applied to two operands.
    Binary {
        /// The operator.
        op: BinaryOp,
        /// The left-hand operand.
        lhs: Box<Expr>,
        /// The right-hand operand.
        rhs: Box<Expr>,
    },
}

impl Expr {
    /// The dispatch kind of this expression, used to select the target's
    /// `### expr` `When`-table row (via the `expr is <kind>` fact).
    pub fn kind(&self) -> ExprKind {
        match self {
            Expr::IntLiteral(_) => ExprKind::Int,
            Expr::FloatLiteral(_) => ExprKind::Float,
            Expr::BoolLiteral(_) => ExprKind::Bool,
            Expr::StringLiteral(_) => ExprKind::String,
            Expr::CharLiteral(_) => ExprKind::Char,
            Expr::NullLiteral => ExprKind::Null,
            Expr::Ref(_) => ExprKind::Ref,
            Expr::Field { .. } => ExprKind::Field,
            Expr::Index { .. } => ExprKind::Index,
            Expr::Call { .. } => ExprKind::Call,
            Expr::Unary { .. } => ExprKind::Unary,
            Expr::Binary { .. } => ExprKind::Binary,
        }
    }
}

/// The dispatch kind of an [`Expr`], answering the `expr is <kind>` fact.
///
/// This is a closed set that mirrors the [`Expr`] variants one-to-one, so a
/// language definition can render each kind through a single `### expr`
/// `When`-table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExprKind {
    /// An integer literal.
    Int,
    /// A float literal.
    Float,
    /// A boolean literal.
    Bool,
    /// A string literal.
    String,
    /// A character literal.
    Char,
    /// The `null` literal.
    Null,
    /// A variable reference.
    Ref,
    /// Field access.
    Field,
    /// Indexing.
    Index,
    /// A call.
    Call,
    /// A unary operator expression.
    Unary,
    /// A binary operator expression.
    Binary,
}

impl ExprKind {
    /// The canonical `expr is <kind>` value spelling for this kind.
    pub fn as_str(self) -> &'static str {
        match self {
            ExprKind::Int => "int",
            ExprKind::Float => "float",
            ExprKind::Bool => "bool",
            ExprKind::String => "string",
            ExprKind::Char => "char",
            ExprKind::Null => "null",
            ExprKind::Ref => "ref",
            ExprKind::Field => "field",
            ExprKind::Index => "index",
            ExprKind::Call => "call",
            ExprKind::Unary => "unary",
            ExprKind::Binary => "binary",
        }
    }
}

/// A unary operator.
///
/// This is the CLOSED kernel unary-operator set. Exotic/sugar operators (e.g.
/// the try `?` postfix) are LAYER concerns and are deliberately excluded. Each
/// operator is capability-gated per target exactly like a primitive: a target
/// may forbid one it lacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnaryOp {
    /// Arithmetic negation (`-`).
    Neg,
    /// Logical NOT (`!`).
    Not,
    /// Bitwise NOT (`~`).
    BitNot,
    /// Unary plus (`+`) — usually identity; kept for completeness.
    Pos,
}

impl UnaryOp {
    /// The canonical Lamina spelling of this operator.
    pub fn as_str(self) -> &'static str {
        match self {
            UnaryOp::Neg => "-",
            UnaryOp::Not => "!",
            UnaryOp::BitNot => "~",
            UnaryOp::Pos => "+",
        }
    }

    /// A stable machine name for this operator (used for capability lookup and
    /// diagnostics), distinct from the symbolic [`UnaryOp::as_str`] spelling.
    pub fn name(self) -> &'static str {
        match self {
            UnaryOp::Neg => "neg",
            UnaryOp::Not => "not",
            UnaryOp::BitNot => "bitnot",
            UnaryOp::Pos => "pos",
        }
    }

    /// Resolves a unary operator from its stable machine name (see
    /// [`UnaryOp::name`]).
    pub fn from_name(name: &str) -> Option<UnaryOp> {
        UnaryOp::all().into_iter().find(|op| op.name() == name)
    }

    /// All unary operators in the closed kernel set, in canonical order.
    pub fn all() -> [UnaryOp; 4] {
        [UnaryOp::Neg, UnaryOp::Not, UnaryOp::BitNot, UnaryOp::Pos]
    }
}

/// A binary operator.
///
/// This is the CLOSED kernel binary-operator set: a finite, capability-gated
/// superset of the semantically-distinct primitive operators across targets.
/// Exotic/sugar operators (matmul `@`, ranges `..`/`...`, try `?`, optional
/// chaining `?.`, the comma operator) are LAYER concerns and are deliberately
/// excluded. Each operator is gated per target exactly like a primitive: a
/// target may forbid one it lacks (e.g. `UShr` `>>>` on a target with no
/// unsigned/logical right shift).
///
/// **Precedence and associativity are not stored.** The AST is already a tree,
/// so grouping is structural. The emitter parenthesizes compound operands to
/// preserve grouping safely (a precedence-minimal printer is a later
/// refinement).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinaryOp {
    /// Addition (`+`).
    Add,
    /// Subtraction (`-`).
    Sub,
    /// Multiplication (`*`).
    Mul,
    /// Division (`/`).
    Div,
    /// Remainder (`%`).
    Rem,
    /// Exponentiation (`**`).
    Pow,
    /// Floor division (`//`).
    FloorDiv,
    /// Equality (`==`).
    Eq,
    /// Inequality (`!=`).
    Ne,
    /// Less-than (`<`).
    Lt,
    /// Less-than-or-equal (`<=`).
    Le,
    /// Greater-than (`>`).
    Gt,
    /// Greater-than-or-equal (`>=`).
    Ge,
    /// Logical AND (`&&`).
    And,
    /// Logical OR (`||`).
    Or,
    /// Bitwise AND (`&`).
    BitAnd,
    /// Bitwise OR (`|`).
    BitOr,
    /// Bitwise XOR (`^`).
    BitXor,
    /// Left shift (`<<`).
    Shl,
    /// Arithmetic/signed right shift (`>>`).
    Shr,
    /// Unsigned/logical right shift (`>>>`) — targets without it forbid it.
    UShr,
}

impl BinaryOp {
    /// The canonical Lamina spelling of this operator.
    pub fn as_str(self) -> &'static str {
        match self {
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            BinaryOp::Rem => "%",
            BinaryOp::Pow => "**",
            BinaryOp::FloorDiv => "//",
            BinaryOp::Eq => "==",
            BinaryOp::Ne => "!=",
            BinaryOp::Lt => "<",
            BinaryOp::Le => "<=",
            BinaryOp::Gt => ">",
            BinaryOp::Ge => ">=",
            BinaryOp::And => "&&",
            BinaryOp::Or => "||",
            BinaryOp::BitAnd => "&",
            BinaryOp::BitOr => "|",
            BinaryOp::BitXor => "^",
            BinaryOp::Shl => "<<",
            BinaryOp::Shr => ">>",
            BinaryOp::UShr => ">>>",
        }
    }

    /// A stable machine name for this operator (used for capability lookup and
    /// diagnostics), distinct from the symbolic [`BinaryOp::as_str`] spelling.
    pub fn name(self) -> &'static str {
        match self {
            BinaryOp::Add => "add",
            BinaryOp::Sub => "sub",
            BinaryOp::Mul => "mul",
            BinaryOp::Div => "div",
            BinaryOp::Rem => "rem",
            BinaryOp::Pow => "pow",
            BinaryOp::FloorDiv => "floordiv",
            BinaryOp::Eq => "eq",
            BinaryOp::Ne => "ne",
            BinaryOp::Lt => "lt",
            BinaryOp::Le => "le",
            BinaryOp::Gt => "gt",
            BinaryOp::Ge => "ge",
            BinaryOp::And => "and",
            BinaryOp::Or => "or",
            BinaryOp::BitAnd => "bitand",
            BinaryOp::BitOr => "bitor",
            BinaryOp::BitXor => "bitxor",
            BinaryOp::Shl => "shl",
            BinaryOp::Shr => "shr",
            BinaryOp::UShr => "ushr",
        }
    }

    /// Resolves a binary operator from its stable machine name (see
    /// [`BinaryOp::name`]).
    pub fn from_name(name: &str) -> Option<BinaryOp> {
        BinaryOp::all().into_iter().find(|op| op.name() == name)
    }

    /// All binary operators in the closed kernel set, in canonical order.
    pub fn all() -> [BinaryOp; 21] {
        [
            BinaryOp::Add,
            BinaryOp::Sub,
            BinaryOp::Mul,
            BinaryOp::Div,
            BinaryOp::Rem,
            BinaryOp::Pow,
            BinaryOp::FloorDiv,
            BinaryOp::Eq,
            BinaryOp::Ne,
            BinaryOp::Lt,
            BinaryOp::Le,
            BinaryOp::Gt,
            BinaryOp::Ge,
            BinaryOp::And,
            BinaryOp::Or,
            BinaryOp::BitAnd,
            BinaryOp::BitOr,
            BinaryOp::BitXor,
            BinaryOp::Shl,
            BinaryOp::Shr,
            BinaryOp::UShr,
        ]
    }
}

/// The kind of construct scope a slot is resolved within.
///
/// A slot name's binding depends on scope: `name` binds to the function's name
/// at function scope, but to the parameter's name within a parameter element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotScope {
    /// Resolving slots of the function declaration itself.
    Function,
    /// Resolving slots of a single parameter element.
    Param,
    /// Resolving slots of a single statement element.
    ///
    /// A statement dispatches on its [`StatementKind`] (via the `stmt is
    /// <kind>` fact) to the `### statement` `When`-table, whose rows reference
    /// this scope's sub-slots. Which sub-slots are meaningful depends on the
    /// kind: an `if` exposes `cond` (expr), `then` (a statement sequence), and
    /// `else` (an optional nested statement); a `let` exposes `name`,
    /// `let_type` (optional), and `value` (optional expr); a `foreach` exposes
    /// `binding`, `iterable`, and `body`; a `switch` exposes `scrutinee`,
    /// `cases` (a sequence), and `default`. Optional sub-parts are guarded by
    /// the `has_value` / `has_else` / `has_type` / `has_default` boolean facts.
    Statement,
    /// Resolving slots of a compound [`Type`] (a `Pointer` or `FnPtr`): the
    /// `pointee` of a pointer, or the `params`/`ret` of a function pointer.
    Type,
    /// Resolving slots of a single function-pointer parameter-type element.
    TypeParam,
    /// Resolving slots of a single expression.
    ///
    /// An expression dispatches on its [`ExprKind`] (via the `expr is <kind>`
    /// fact) to a `### expr` `When`-table, whose rows reference this scope's
    /// sub-slots: `op` (operator spelling), `lhs`/`rhs`/`operand`/`obj`/`index`/
    /// `callee` (nested expressions), `args` (a sequence of expressions),
    /// `field` (a name), and `value` (a literal's textual contents).
    Expr,
    /// Resolving slots of a single call-argument element (one element of a
    /// [`Expr::Call`]'s `args`). Its `value` sub-slot renders the argument
    /// expression; loop facts (`first`/`last`) let the item template supply its
    /// own separator.
    ExprArg,
    /// Resolving slots of a single [`SwitchCase`] element (one element of a
    /// [`Statement::Switch`]'s `cases`). Exposes `value` (the matched
    /// expression) and `body` (the case's statement sequence); loop facts let
    /// the item template supply its own separator.
    SwitchCase,
    /// Resolving slots of a [`Item::Struct`] declaration itself: its `name`
    /// (scalar) and `fields` (a sequence looping the `field` item slot).
    Struct,
    /// Resolving slots of a single [`Field`] element (one element of a struct's
    /// `fields`). Exposes `name` and `type`; loop facts let the item template
    /// supply its own separator.
    Field,
    /// Resolving slots of a [`Item::Enum`] declaration itself: its `name`
    /// (scalar) and `variants` (a sequence looping the `variant` item slot).
    Enum,
    /// Resolving slots of a single [`Variant`] element (one element of an
    /// enum's `variants`). Exposes `name`; loop facts let the item template
    /// supply its own separator.
    Variant,
    /// Resolving slots of a [`Item::TypeDef`] declaration: its `name` and
    /// `target` (the aliased type, rendered as a scalar).
    TypeDef,
    /// Resolving slots of a [`Item::Const`] declaration: its `name`, `type`,
    /// and `value` (all scalar).
    Const,
    /// Resolving slots of a [`Item::Use`] declaration: its `path` (scalar).
    Use,
}

/// The shape of an engine-bound slot: how the AST field it maps to is rendered.
///
/// This is the single source of truth for slot cardinality. It is derived from
/// the *shape of the AST field* the slot binds to — a scalar field renders
/// directly; a sequence field (a `Vec`) loops an item slot. There is no
/// hardcoded list of "terminal" or "collection" slots: cardinality is a
/// property of the bound data, queried via [`slot_binding`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotShape {
    /// The slot binds to a scalar AST value and renders directly.
    Scalar,
    /// The slot binds to a sequence AST field; the engine loops it, rendering
    /// the named item slot once per element (with `first`/`last` loop facts).
    Sequence {
        /// The item slot rendered per element (e.g. `params` -> `param`).
        item_slot: String,
        /// The scope the item slot's own slots resolve in (e.g. `param`
        /// elements resolve in [`SlotScope::Param`]).
        item_scope: SlotScope,
    },
}

/// Returns the [`SlotShape`] of an engine-bound slot `name` within `scope`, or
/// `None` if the name is not an engine-bound AST slot (in which case it must be
/// satisfied by a `### <slot>` subsection in the language definition).
///
/// This is the *only* place slot-to-AST bindings live. Adding a new AST field
/// (e.g. a `struct`'s fields) means adding a binding here; a `Vec` field is a
/// `Sequence`, a scalar field is `Scalar` — cardinality follows the field type.
pub fn slot_binding(name: &str, scope: SlotScope) -> Option<SlotShape> {
    match scope {
        SlotScope::Function => match name {
            "name" => Some(SlotShape::Scalar),
            "ret_type" => Some(SlotShape::Scalar),
            "params" => Some(SlotShape::Sequence {
                item_slot: "param".to_string(),
                item_scope: SlotScope::Param,
            }),
            "body" => Some(SlotShape::Sequence {
                item_slot: "statement".to_string(),
                item_scope: SlotScope::Statement,
            }),
            _ => None,
        },
        SlotScope::Param => match name {
            "name" => Some(SlotShape::Scalar),
            "type" => Some(SlotShape::Scalar),
            _ => None,
        },
        // A statement's sub-slots. Which are meaningful depends on the
        // statement kind, but cardinality is uniform per name: nested single
        // expressions (`cond`, `value`, `iterable`, `scrutinee`) and nested
        // single statements (`else`, `init`, `step`) are scalar; names
        // (`name`, `binding`, `let_type`) are scalar leaves; and the nested
        // statement sequences (`then`, `body`, `default`) loop the `statement`
        // item slot recursively, while `cases` loops the `switch_case` item
        // slot.
        SlotScope::Statement => match name {
            "name" => Some(SlotShape::Scalar),
            "binding" => Some(SlotShape::Scalar),
            "let_type" => Some(SlotShape::Scalar),
            "value" => Some(SlotShape::Scalar),
            "cond" => Some(SlotShape::Scalar),
            "iterable" => Some(SlotShape::Scalar),
            "scrutinee" => Some(SlotShape::Scalar),
            "else" => Some(SlotShape::Scalar),
            "init" => Some(SlotShape::Scalar),
            "step" => Some(SlotShape::Scalar),
            "then" => Some(SlotShape::Sequence {
                item_slot: "statement".to_string(),
                item_scope: SlotScope::Statement,
            }),
            "body" => Some(SlotShape::Sequence {
                item_slot: "statement".to_string(),
                item_scope: SlotScope::Statement,
            }),
            "default" => Some(SlotShape::Sequence {
                item_slot: "statement".to_string(),
                item_scope: SlotScope::Statement,
            }),
            "cases" => Some(SlotShape::Sequence {
                item_slot: "switch_case".to_string(),
                item_scope: SlotScope::SwitchCase,
            }),
            _ => None,
        },
        // A single switch-case element: `value` is the matched expression,
        // `body` the case's statement sequence.
        SlotScope::SwitchCase => match name {
            "value" => Some(SlotShape::Scalar),
            "body" => Some(SlotShape::Sequence {
                item_slot: "statement".to_string(),
                item_scope: SlotScope::Statement,
            }),
            _ => None,
        },
        // A compound type's sub-slots. `pointee` and `ret` are single nested
        // types (scalar); `params` is a sequence of parameter types, each
        // rendered via the `type_param` item slot.
        SlotScope::Type => match name {
            "pointee" => Some(SlotShape::Scalar),
            "ret" => Some(SlotShape::Scalar),
            "params" => Some(SlotShape::Sequence {
                item_slot: "type_param".to_string(),
                item_scope: SlotScope::TypeParam,
            }),
            _ => None,
        },
        // A single function-pointer parameter type: `type` is the rendered
        // element type.
        SlotScope::TypeParam => match name {
            "type" => Some(SlotShape::Scalar),
            _ => None,
        },
        // An expression's sub-slots. Nested expressions (`lhs`, `rhs`,
        // `operand`, `obj`, `index`, `callee`) are scalar (each a single child
        // expression rendered via the `### expr` dispatch table). `op` is the
        // operator's spelling, `field` a name, `value` a literal's textual
        // contents — all scalar leaves. `args` is a sequence of argument
        // expressions, each rendered via the `expr_arg` item slot.
        SlotScope::Expr => match name {
            "op" => Some(SlotShape::Scalar),
            "value" => Some(SlotShape::Scalar),
            "field" => Some(SlotShape::Scalar),
            "operand" => Some(SlotShape::Scalar),
            "lhs" => Some(SlotShape::Scalar),
            "rhs" => Some(SlotShape::Scalar),
            "obj" => Some(SlotShape::Scalar),
            "index" => Some(SlotShape::Scalar),
            "callee" => Some(SlotShape::Scalar),
            "args" => Some(SlotShape::Sequence {
                item_slot: "expr_arg".to_string(),
                item_scope: SlotScope::ExprArg,
            }),
            _ => None,
        },
        // A single call-argument element: `value` renders the argument
        // expression (dispatching through the `### expr` table).
        SlotScope::ExprArg => match name {
            "value" => Some(SlotShape::Scalar),
            _ => None,
        },
        // A struct declaration: `name` is a scalar leaf; `fields` loops the
        // `field` item slot.
        SlotScope::Struct => match name {
            "name" => Some(SlotShape::Scalar),
            "fields" => Some(SlotShape::Sequence {
                item_slot: "field".to_string(),
                item_scope: SlotScope::Field,
            }),
            _ => None,
        },
        // A single struct-field element: `name` and `type` are scalar leaves
        // (`type` resolves the field's declared type).
        SlotScope::Field => match name {
            "name" => Some(SlotShape::Scalar),
            "type" => Some(SlotShape::Scalar),
            _ => None,
        },
        // An enum declaration: `name` is a scalar leaf; `variants` loops the
        // `variant` item slot.
        SlotScope::Enum => match name {
            "name" => Some(SlotShape::Scalar),
            "variants" => Some(SlotShape::Sequence {
                item_slot: "variant".to_string(),
                item_scope: SlotScope::Variant,
            }),
            _ => None,
        },
        // A single enum-variant element: `name` is a scalar leaf. (Payloads are
        // deferred, so a variant exposes only its name for now.)
        SlotScope::Variant => match name {
            "name" => Some(SlotShape::Scalar),
            _ => None,
        },
        // A type alias: `name` is a scalar leaf; `target` renders the aliased
        // type (scalar, dispatched through the type machinery).
        SlotScope::TypeDef => match name {
            "name" => Some(SlotShape::Scalar),
            "target" => Some(SlotShape::Scalar),
            _ => None,
        },
        // An item-level constant: `name`, `type`, and `value` are scalar leaves
        // (`type` resolves the declared type, `value` renders the value
        // expression through the `### expr` table).
        SlotScope::Const => match name {
            "name" => Some(SlotShape::Scalar),
            "type" => Some(SlotShape::Scalar),
            "value" => Some(SlotShape::Scalar),
            _ => None,
        },
        // A `use` import: `path` is a scalar leaf rendered verbatim.
        SlotScope::Use => match name {
            "path" => Some(SlotShape::Scalar),
            _ => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unary_op_roundtrips_via_name() {
        for op in UnaryOp::all() {
            assert_eq!(UnaryOp::from_name(op.name()), Some(op));
            // `as_str` is the symbolic spelling; `name` is the machine name.
            assert!(!op.as_str().is_empty());
        }
        assert_eq!(UnaryOp::from_name("nope"), None);
    }

    #[test]
    fn unary_op_spellings_are_canonical() {
        assert_eq!(UnaryOp::Neg.as_str(), "-");
        assert_eq!(UnaryOp::Not.as_str(), "!");
        assert_eq!(UnaryOp::BitNot.as_str(), "~");
        assert_eq!(UnaryOp::Pos.as_str(), "+");
    }

    #[test]
    fn binary_op_roundtrips_via_name() {
        for op in BinaryOp::all() {
            assert_eq!(BinaryOp::from_name(op.name()), Some(op));
            assert!(!op.as_str().is_empty());
        }
        assert_eq!(BinaryOp::from_name("matmul"), None);
    }

    #[test]
    fn binary_op_set_is_the_closed_kernel_set() {
        // The spec fixes exactly 21 binary operators; guard against accidental
        // addition of exotic/sugar operators.
        assert_eq!(BinaryOp::all().len(), 21);
        // Spot-check the semantically notable ones.
        assert_eq!(BinaryOp::Pow.as_str(), "**");
        assert_eq!(BinaryOp::FloorDiv.as_str(), "//");
        assert_eq!(BinaryOp::UShr.as_str(), ">>>");
        assert_eq!(BinaryOp::Shr.as_str(), ">>");
        assert_eq!(BinaryOp::Shl.as_str(), "<<");
    }

    #[test]
    fn unary_op_set_is_the_closed_kernel_set() {
        assert_eq!(UnaryOp::all().len(), 4);
    }

    #[test]
    fn operator_names_are_unique() {
        let mut names: Vec<&str> = BinaryOp::all().iter().map(|o| o.name()).collect();
        names.extend(UnaryOp::all().iter().map(|o| o.name()));
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "operator machine names must be unique");
    }

    #[test]
    fn expr_kind_matches_variant() {
        assert_eq!(Expr::IntLiteral("1".into()).kind(), ExprKind::Int);
        assert_eq!(Expr::FloatLiteral("1.0".into()).kind(), ExprKind::Float);
        assert_eq!(Expr::BoolLiteral(true).kind(), ExprKind::Bool);
        assert_eq!(Expr::StringLiteral("s".into()).kind(), ExprKind::String);
        assert_eq!(Expr::CharLiteral("c".into()).kind(), ExprKind::Char);
        assert_eq!(Expr::NullLiteral.kind(), ExprKind::Null);
        assert_eq!(Expr::Ref("x".into()).kind(), ExprKind::Ref);
        let obj = Box::new(Expr::Ref("o".into()));
        assert_eq!(
            Expr::Field {
                obj: obj.clone(),
                field: "f".into()
            }
            .kind(),
            ExprKind::Field
        );
        assert_eq!(
            Expr::Index {
                obj: obj.clone(),
                index: Box::new(Expr::IntLiteral("0".into()))
            }
            .kind(),
            ExprKind::Index
        );
        assert_eq!(
            Expr::Call {
                callee: obj.clone(),
                args: vec![]
            }
            .kind(),
            ExprKind::Call
        );
        assert_eq!(
            Expr::Unary {
                op: UnaryOp::Neg,
                operand: obj.clone()
            }
            .kind(),
            ExprKind::Unary
        );
        assert_eq!(
            Expr::Binary {
                op: BinaryOp::Add,
                lhs: obj.clone(),
                rhs: obj
            }
            .kind(),
            ExprKind::Binary
        );
    }

    #[test]
    fn expr_scope_binds_sub_slots() {
        assert_eq!(slot_binding("op", SlotScope::Expr), Some(SlotShape::Scalar));
        assert_eq!(slot_binding("lhs", SlotScope::Expr), Some(SlotShape::Scalar));
        assert_eq!(slot_binding("rhs", SlotScope::Expr), Some(SlotShape::Scalar));
        assert_eq!(
            slot_binding("operand", SlotScope::Expr),
            Some(SlotShape::Scalar)
        );
        assert_eq!(slot_binding("obj", SlotScope::Expr), Some(SlotShape::Scalar));
        assert_eq!(
            slot_binding("callee", SlotScope::Expr),
            Some(SlotShape::Scalar)
        );
        match slot_binding("args", SlotScope::Expr) {
            Some(SlotShape::Sequence {
                item_slot,
                item_scope,
            }) => {
                assert_eq!(item_slot, "expr_arg");
                assert_eq!(item_scope, SlotScope::ExprArg);
            }
            other => panic!("args should be a sequence, got {other:?}"),
        }
        // An unknown expr slot is not engine-bound.
        assert_eq!(slot_binding("bogus", SlotScope::Expr), None);
        // A call-argument element exposes `value`.
        assert_eq!(
            slot_binding("value", SlotScope::ExprArg),
            Some(SlotShape::Scalar)
        );
    }

    #[test]
    fn statement_kind_matches_variant() {
        assert_eq!(Statement::Block(vec![]).kind(), StatementKind::Block);
        assert_eq!(
            Statement::Let {
                name: "x".into(),
                ty: None,
                value: None
            }
            .kind(),
            StatementKind::Let
        );
        assert_eq!(Statement::Return(None).kind(), StatementKind::Return);
        assert_eq!(
            Statement::If {
                cond: Expr::BoolLiteral(true),
                then_block: vec![],
                else_block: None
            }
            .kind(),
            StatementKind::If
        );
        assert_eq!(
            Statement::While {
                cond: Expr::BoolLiteral(true),
                body: vec![]
            }
            .kind(),
            StatementKind::While
        );
        assert_eq!(
            Statement::For {
                init: None,
                cond: None,
                step: None,
                body: vec![]
            }
            .kind(),
            StatementKind::For
        );
        assert_eq!(
            Statement::ForEach {
                binding: "i".into(),
                iterable: Expr::Ref("xs".into()),
                body: vec![]
            }
            .kind(),
            StatementKind::ForEach
        );
        assert_eq!(
            Statement::Switch {
                scrutinee: Expr::Ref("x".into()),
                cases: vec![],
                default: None
            }
            .kind(),
            StatementKind::Switch
        );
        assert_eq!(Statement::Break.kind(), StatementKind::Break);
        assert_eq!(Statement::Continue.kind(), StatementKind::Continue);
        assert_eq!(
            Statement::Expr(Expr::Ref("x".into())).kind(),
            StatementKind::Expr
        );
    }

    #[test]
    fn statement_kind_set_is_closed() {
        // Exactly 11 statement kinds; guard against accidental drift.
        assert_eq!(StatementKind::all().len(), 11);
        // Spot-check the kind spellings used by the `stmt is <kind>` fact.
        assert_eq!(StatementKind::ForEach.as_str(), "foreach");
        assert_eq!(StatementKind::Switch.as_str(), "switch");
        assert_eq!(StatementKind::Expr.as_str(), "expr");
    }

    #[test]
    fn statement_scope_binds_sub_slots() {
        // Scalar sub-slots.
        for name in [
            "name", "binding", "let_type", "value", "cond", "iterable", "scrutinee", "else",
            "init", "step",
        ] {
            assert_eq!(
                slot_binding(name, SlotScope::Statement),
                Some(SlotShape::Scalar),
                "{name} should be scalar at statement scope"
            );
        }
        // Statement-sequence sub-slots loop the recursive `statement` item.
        for name in ["then", "body", "default"] {
            match slot_binding(name, SlotScope::Statement) {
                Some(SlotShape::Sequence {
                    item_slot,
                    item_scope,
                }) => {
                    assert_eq!(item_slot, "statement");
                    assert_eq!(item_scope, SlotScope::Statement);
                }
                other => panic!("{name} should be a statement sequence, got {other:?}"),
            }
        }
        // `cases` loops the `switch_case` item slot.
        match slot_binding("cases", SlotScope::Statement) {
            Some(SlotShape::Sequence {
                item_slot,
                item_scope,
            }) => {
                assert_eq!(item_slot, "switch_case");
                assert_eq!(item_scope, SlotScope::SwitchCase);
            }
            other => panic!("cases should be a switch_case sequence, got {other:?}"),
        }
        // Unknown statement slot is not engine-bound.
        assert_eq!(slot_binding("bogus", SlotScope::Statement), None);
    }

    #[test]
    fn switch_case_scope_binds_value_and_body() {
        assert_eq!(
            slot_binding("value", SlotScope::SwitchCase),
            Some(SlotShape::Scalar)
        );
        match slot_binding("body", SlotScope::SwitchCase) {
            Some(SlotShape::Sequence {
                item_slot,
                item_scope,
            }) => {
                assert_eq!(item_slot, "statement");
                assert_eq!(item_scope, SlotScope::Statement);
            }
            other => panic!("case body should be a statement sequence, got {other:?}"),
        }
    }

    #[test]
    fn item_kind_matches_variant() {
        assert_eq!(
            Item::Function(Function {
                name: "f".into(),
                visibility: Visibility::Private,
                modifiers: vec![],
                params: vec![],
                return_type: Type::Primitive(Primitive::Void),
                body: vec![],
            })
            .kind(),
            ItemKind::Function
        );
        assert_eq!(
            Item::Struct {
                name: "S".into(),
                visibility: Visibility::Public,
                fields: vec![],
            }
            .kind(),
            ItemKind::Struct
        );
        assert_eq!(
            Item::Enum {
                name: "E".into(),
                visibility: Visibility::Public,
                variants: vec![],
            }
            .kind(),
            ItemKind::Enum
        );
        assert_eq!(
            Item::TypeDef {
                name: "T".into(),
                target: Type::Primitive(Primitive::I32),
            }
            .kind(),
            ItemKind::TypeDef
        );
        assert_eq!(
            Item::Const {
                name: "K".into(),
                ty: Type::Primitive(Primitive::I32),
                value: Expr::IntLiteral("1".into()),
                visibility: Visibility::Public,
            }
            .kind(),
            ItemKind::Const
        );
        assert_eq!(
            Item::Use {
                path: "std::io".into()
            }
            .kind(),
            ItemKind::Use
        );
    }

    #[test]
    fn item_kind_set_is_closed_and_spellings_match_headings() {
        assert_eq!(ItemKind::all().len(), 6);
        // Each `item is <kind>` spelling and its `## <Heading>` are consistent.
        for kind in ItemKind::all() {
            assert!(!kind.as_str().is_empty());
            assert_eq!(kind.heading().to_ascii_lowercase(), kind.as_str());
            assert_eq!(kind.scope(), kind.scope());
        }
        assert_eq!(ItemKind::TypeDef.heading(), "TypeDef");
        assert_eq!(ItemKind::TypeDef.as_str(), "typedef");
    }

    #[test]
    fn item_kind_scope_maps_to_slot_scope() {
        assert_eq!(ItemKind::Function.scope(), SlotScope::Function);
        assert_eq!(ItemKind::Struct.scope(), SlotScope::Struct);
        assert_eq!(ItemKind::Enum.scope(), SlotScope::Enum);
        assert_eq!(ItemKind::TypeDef.scope(), SlotScope::TypeDef);
        assert_eq!(ItemKind::Const.scope(), SlotScope::Const);
        assert_eq!(ItemKind::Use.scope(), SlotScope::Use);
    }

    #[test]
    fn struct_scope_binds_name_and_fields() {
        assert_eq!(
            slot_binding("name", SlotScope::Struct),
            Some(SlotShape::Scalar)
        );
        match slot_binding("fields", SlotScope::Struct) {
            Some(SlotShape::Sequence {
                item_slot,
                item_scope,
            }) => {
                assert_eq!(item_slot, "field");
                assert_eq!(item_scope, SlotScope::Field);
            }
            other => panic!("fields should be a field sequence, got {other:?}"),
        }
        assert_eq!(slot_binding("bogus", SlotScope::Struct), None);
    }

    #[test]
    fn field_scope_binds_name_and_type() {
        assert_eq!(
            slot_binding("name", SlotScope::Field),
            Some(SlotShape::Scalar)
        );
        assert_eq!(
            slot_binding("type", SlotScope::Field),
            Some(SlotShape::Scalar)
        );
        assert_eq!(slot_binding("bogus", SlotScope::Field), None);
    }

    #[test]
    fn enum_scope_binds_name_and_variants() {
        assert_eq!(
            slot_binding("name", SlotScope::Enum),
            Some(SlotShape::Scalar)
        );
        match slot_binding("variants", SlotScope::Enum) {
            Some(SlotShape::Sequence {
                item_slot,
                item_scope,
            }) => {
                assert_eq!(item_slot, "variant");
                assert_eq!(item_scope, SlotScope::Variant);
            }
            other => panic!("variants should be a variant sequence, got {other:?}"),
        }
    }

    #[test]
    fn variant_scope_binds_name_only() {
        assert_eq!(
            slot_binding("name", SlotScope::Variant),
            Some(SlotShape::Scalar)
        );
        // Payloads are deferred: a variant exposes only its name.
        assert_eq!(slot_binding("value", SlotScope::Variant), None);
    }

    #[test]
    fn typedef_const_use_scopes_bind_scalars() {
        assert_eq!(
            slot_binding("name", SlotScope::TypeDef),
            Some(SlotShape::Scalar)
        );
        assert_eq!(
            slot_binding("target", SlotScope::TypeDef),
            Some(SlotShape::Scalar)
        );
        for name in ["name", "type", "value"] {
            assert_eq!(
                slot_binding(name, SlotScope::Const),
                Some(SlotShape::Scalar),
                "{name} should be scalar at const scope"
            );
        }
        assert_eq!(
            slot_binding("path", SlotScope::Use),
            Some(SlotShape::Scalar)
        );
        assert_eq!(slot_binding("bogus", SlotScope::Use), None);
    }
}
