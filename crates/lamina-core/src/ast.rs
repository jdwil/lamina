//! The Lamina abstract syntax tree (AST).
//!
//! This is the in-memory representation of parsed Lamina IR. It is deliberately
//! minimal for the first end-to-end slice: a file containing functions, where a
//! function returns a single integer literal. The AST is *structure-preserving*
//! and *target-agnostic* — it carries no knowledge of any target language. All
//! target-specific decisions live in a [`crate::lang`] language definition.

/// An open, engine-transparent key/value channel a layer attaches to a
/// construct to communicate *intent* to a language definition.
///
/// # The engine is transparent to metadata
///
/// The engine defines **no** keys, validates nothing, standardizes nothing, and
/// has no opinion about any key's meaning. There is **no** blessed metadata
/// vocabulary — keys are entirely open and are a contract between a *layer* and
/// a *language definition*, documented by their authors, never by the engine.
/// The engine only carries metadata through the IR, exposes it to the language
/// definition for lookup (the `has_meta(<key>)` / `meta.<key> is <value>` facts
/// and the `{meta.<key>}` slot), and otherwise ignores it.
///
/// # Determinism
///
/// Backed by a [`BTreeMap`](std::collections::BTreeMap) so iteration order is
/// stable (sorted by key), keeping any metadata-derived output deterministic.
///
/// # Structural equality ignores metadata
///
/// [`Meta`] deliberately does **not** participate in structural equality of the
/// nodes that carry it: two constructs are structurally equal iff their
/// *structure* matches, regardless of metadata. The AST nodes hand-implement
/// [`PartialEq`] to skip their `meta` field (see e.g. [`Expr`]), so metadata can
/// never perturb the `eq` structural predicate or idiom matching.
///
/// A node with empty metadata renders byte-identically to one built before
/// metadata existed — [`Meta::default`] is empty and adds nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Meta {
    /// The entries, keyed by an open string key, stored sorted for determinism.
    entries: std::collections::BTreeMap<String, String>,
}

impl Meta {
    /// An empty metadata map (the default for every construct).
    pub fn new() -> Self {
        Meta::default()
    }

    /// Builds a metadata map from key/value pairs. Convenience for layers (and
    /// tests) that tag a construct at construction time.
    pub fn from_pairs<K, V, I>(pairs: I) -> Self
    where
        K: Into<String>,
        V: Into<String>,
        I: IntoIterator<Item = (K, V)>,
    {
        Meta {
            entries: pairs
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        }
    }

    /// Sets a key/value pair, returning `self` for chaining.
    #[must_use]
    pub fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.entries.insert(key.into(), value.into());
        self
    }

    /// Inserts a key/value pair in place.
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.entries.insert(key.into(), value.into());
    }

    /// Returns `true` if the map has an entry for `key` (answers the
    /// `has_meta(<key>)` fact).
    pub fn has(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    /// Returns the value for `key`, if present (answers `meta.<key> is <value>`
    /// and the `{meta.<key>}` slot).
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }

    /// Returns `true` if there is no metadata.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterates the entries in deterministic (sorted-by-key) order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

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
#[derive(Debug, Clone)]
pub enum Item {
    /// A function definition (see [`Function`]). The function's own metadata
    /// lives on the [`Function`] struct.
    Function(Function),
    /// A `struct`: a named aggregate of typed fields.
    Struct {
        /// The struct's identifier.
        name: String,
        /// The struct's visibility.
        visibility: Visibility,
        /// The fields, in declaration order (possibly empty).
        fields: Vec<Field>,
        /// The type-level attributes applied to this struct, in source order
        /// (see [`TypeAttribute`]); default empty. The kernel reserves the
        /// superset of derivable capabilities; a language definition realizes
        /// or forbids each one per target. Like [`Meta`], attributes do **not**
        /// participate in structural equality (see the [`Item`] `PartialEq`
        /// impl), so a compound-assign structural comparison is unaffected.
        attributes: Vec<TypeAttribute>,
        /// Engine-transparent metadata (see [`Meta`]); default empty.
        meta: Meta,
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
        /// The type-level attributes applied to this enum, in source order
        /// (see [`TypeAttribute`]); default empty. Realized or forbidden per
        /// target exactly as for a struct; ignored by structural equality.
        attributes: Vec<TypeAttribute>,
        /// Engine-transparent metadata (see [`Meta`]); default empty.
        meta: Meta,
    },
    /// A type alias (`typedef`): a name bound to a target [`Type`].
    TypeDef {
        /// The alias's identifier.
        name: String,
        /// The aliased type.
        target: Type,
        /// Engine-transparent metadata (see [`Meta`]); default empty.
        meta: Meta,
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
        /// Engine-transparent metadata (see [`Meta`]); default empty.
        meta: Meta,
    },
    /// An import (`use path;`).
    ///
    /// The `path` is the module path, always present. Two optional refinements
    /// layer on top:
    /// - **selective import** (`use path::{a, b as c}`): `items` is populated
    ///   with the imported [`UseItem`]s (each optionally aliased);
    /// - **module alias** (`use path as p`): `alias` is set.
    ///
    /// A bare module import (`items` empty, `alias` `None`) renders exactly as
    /// before — the `has_items` / `has_alias` facts are both false, so the
    /// target's `### use` (or entry) dispatch picks the plain `use {path};`
    /// form and the output is byte-identical to the pre-structured form.
    Use {
        /// The imported module path, verbatim.
        path: String,
        /// The selectively-imported items (`use path::{a, b as c}`); empty for a
        /// bare or module-aliased import.
        items: Vec<UseItem>,
        /// A module alias (`use path as p`); `None` for a bare or selective
        /// import.
        alias: Option<String>,
        /// Engine-transparent metadata (see [`Meta`]); default empty.
        meta: Meta,
    },
    /// A **top-level tree value** (the tree core): a file whose root *is* a
    /// declarative tree, i.e. a pure config/markup document (an HTML page, a
    /// JSON object). The inner [`Expr`] is a tree expression — typically an
    /// [`Expr::Node`], but any expression is permitted so the two cores
    /// interoperate uniformly. Unlike the other item kinds, a tree item has no
    /// entry template of its own: it renders straight through the shared
    /// `### expr` dispatch, exactly as a tree expression embedded in imperative
    /// code would.
    Tree(Expr),
    /// A **raw / verbatim top-level item**: a string of literal target code the
    /// engine emits UNCHANGED (the ultimate layer escape hatch).
    ///
    /// Like [`Expr::Raw`] and [`Statement::Raw`], a raw item is NOT
    /// target-keyed: it exists in the AST only when a layer lowered it *for the
    /// current target*, so the engine passes the string through with no target
    /// check, capability gating, or error path. It renders through the target's
    /// `## Raw` item section (a trivial pass-through entry template that emits
    /// the `value` scalar slot), consistent with how every other item kind
    /// dispatches to its own `## <Item>` section. Structural equality compares
    /// the string and ignores `meta`.
    Raw {
        /// The verbatim target-code fragment, emitted unchanged.
        code: String,
        /// Engine-transparent metadata (see [`Meta`]); default empty. A raw
        /// item is raiseable like any other node.
        meta: Meta,
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
            Item::Tree(_) => ItemKind::Tree,
            Item::Raw { .. } => ItemKind::Raw,
        }
    }

    /// This item's engine-transparent [`Meta`]. For [`Item::Function`] the
    /// metadata is carried on the [`Function`] struct; every other variant
    /// carries it inline.
    pub fn meta(&self) -> &Meta {
        match self {
            Item::Function(f) => &f.meta,
            Item::Struct { meta, .. }
            | Item::Enum { meta, .. }
            | Item::TypeDef { meta, .. }
            | Item::Const { meta, .. }
            | Item::Use { meta, .. } => meta,
            // A tree item's metadata is the tree expression's own metadata.
            Item::Tree(expr) => expr.meta(),
            Item::Raw { meta, .. } => meta,
        }
    }
}

/// Structural equality of items **ignores metadata** (see [`Meta`]) **and
/// type attributes** (see [`TypeAttribute`]): two items are equal iff their
/// structure matches, regardless of what a layer tagged or which type-level
/// attributes were requested. Keeping attributes out of equality preserves the
/// compound-assign structural predicate exactly as ignoring metadata does.
impl PartialEq for Item {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Item::Function(a), Item::Function(b)) => a == b,
            (
                Item::Struct {
                    name: an,
                    visibility: av,
                    fields: af,
                    ..
                },
                Item::Struct {
                    name: bn,
                    visibility: bv,
                    fields: bf,
                    ..
                },
            ) => an == bn && av == bv && af == bf,
            (
                Item::Enum {
                    name: an,
                    visibility: av,
                    variants: avr,
                    ..
                },
                Item::Enum {
                    name: bn,
                    visibility: bv,
                    variants: bvr,
                    ..
                },
            ) => an == bn && av == bv && avr == bvr,
            (
                Item::TypeDef {
                    name: an,
                    target: at,
                    ..
                },
                Item::TypeDef {
                    name: bn,
                    target: bt,
                    ..
                },
            ) => an == bn && at == bt,
            (
                Item::Const {
                    name: an,
                    ty: at,
                    value: aval,
                    visibility: av,
                    ..
                },
                Item::Const {
                    name: bn,
                    ty: bt,
                    value: bval,
                    visibility: bv,
                    ..
                },
            ) => an == bn && at == bt && aval == bval && av == bv,
            (
                Item::Use {
                    path: ap,
                    items: ai,
                    alias: aa,
                    ..
                },
                Item::Use {
                    path: bp,
                    items: bi,
                    alias: ba,
                    ..
                },
            ) => ap == bp && ai == bi && aa == ba,
            (Item::Tree(a), Item::Tree(b)) => a == b,
            // A raw item's structural identity is its verbatim string; metadata
            // is ignored (consistent with every other node).
            (Item::Raw { code: a, .. }, Item::Raw { code: b, .. }) => a == b,
            _ => false,
        }
    }
}

impl Eq for Item {}

/// A `struct` field: a name, a type, and a visibility.
///
/// Structural equality **ignores** the `meta` field (see [`Meta`]).
#[derive(Debug, Clone)]
pub struct Field {
    /// The field's identifier.
    pub name: String,
    /// The field's declared type.
    pub ty: Type,
    /// The field's visibility.
    pub visibility: Visibility,
    /// Engine-transparent metadata (see [`Meta`]); default empty. A closure's
    /// capture-environment field may be tagged (e.g. `capture=true`), or an
    /// anonymous class's method field tagged (e.g. `member=method`).
    pub meta: Meta,
}

impl PartialEq for Field {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.ty == other.ty && self.visibility == other.visibility
    }
}

impl Eq for Field {}

/// One imported item of a selective [`Item::Use`] (`use path::{name, name as
/// alias}`): a name and an optional local alias.
///
/// Structural equality **ignores** the `meta` field (see [`Meta`]).
#[derive(Debug, Clone)]
pub struct UseItem {
    /// The imported item's name (as it appears in the source module).
    pub name: String,
    /// An optional local alias (`name as alias`); `None` imports the item under
    /// its own name.
    pub alias: Option<String>,
    /// Engine-transparent metadata (see [`Meta`]); default empty.
    pub meta: Meta,
}

impl PartialEq for UseItem {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.alias == other.alias
    }
}

impl Eq for UseItem {}

/// An `enum` variant: a name and an optional payload (associated data).
///
/// A variant may be **unit** (no payload, a plain named enumerator — the C-like
/// form that renders everywhere), **tuple**-style (an ordered list of unnamed
/// payload types, e.g. Rust `Some(T)`), or **struct**-style (named payload
/// fields, e.g. Rust `Point { x: i32, y: i32 }`). See [`VariantPayload`].
///
/// Payloads are **capability-gated** in the language definition: a target that
/// lacks tagged unions (a C-style enum) forbids the tuple/struct rows of its
/// `### variant` slot, so a payload-bearing variant surfaces a clear
/// forbidden-construct error there while a unit variant still renders.
///
/// Structural equality **ignores** the `meta` field (see [`Meta`]).
#[derive(Debug, Clone)]
pub struct Variant {
    /// The variant's identifier.
    pub name: String,
    /// The variant's payload (associated data), or [`VariantPayload::None`] for
    /// a plain unit variant.
    pub payload: VariantPayload,
    /// Engine-transparent metadata (see [`Meta`]); default empty.
    pub meta: Meta,
}

impl PartialEq for Variant {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.payload == other.payload
    }
}

impl Eq for Variant {}

/// The payload (associated data) carried by an [`Variant`].
///
/// This is the closed kernel set of variant shapes. A target's `### variant`
/// slot dispatches on the shape via the `variant is unit|tuple|struct` fact:
/// - [`VariantPayload::None`] — a plain unit variant (`variant is unit`);
/// - [`VariantPayload::Tuple`] — ordered unnamed payload types
///   (`variant is tuple`), rendered via the `payload_types` sequence slot
///   (item slot `payload_type`);
/// - [`VariantPayload::Struct`] — named payload fields (`variant is struct`),
///   rendered via the `payload_fields` sequence slot (item slot
///   `payload_field`).
///
/// Structural equality is derived (the contained [`Field`]/[`Type`] both
/// already ignore metadata in their own equality).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VariantPayload {
    /// No payload — a plain unit variant (the C-like enumerator form).
    None,
    /// A tuple-style payload: an ordered list of unnamed types.
    Tuple(Vec<Type>),
    /// A struct-style payload: named fields.
    Struct(Vec<Field>),
}

impl VariantPayload {
    /// The dispatch kind of this payload, for the `variant is <kind>` fact.
    pub fn kind(&self) -> VariantKind {
        match self {
            VariantPayload::None => VariantKind::Unit,
            VariantPayload::Tuple(_) => VariantKind::Tuple,
            VariantPayload::Struct(_) => VariantKind::Struct,
        }
    }
}

/// The dispatch kind of a [`Variant`]'s payload, answering `variant is <kind>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariantKind {
    /// A unit variant (no payload).
    Unit,
    /// A tuple-style payload.
    Tuple,
    /// A struct-style payload.
    Struct,
}

impl VariantKind {
    /// The canonical `variant is <kind>` value spelling for this kind.
    pub fn as_str(self) -> &'static str {
        match self {
            VariantKind::Unit => "unit",
            VariantKind::Tuple => "tuple",
            VariantKind::Struct => "struct",
        }
    }
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
    /// A top-level tree value (the tree core).
    Tree,
    /// A raw / verbatim top-level item (the layer escape hatch).
    Raw,
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
            ItemKind::Tree => "tree",
            ItemKind::Raw => "raw",
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
            ItemKind::Tree => "Tree",
            ItemKind::Raw => "Raw",
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
            ItemKind::Tree => SlotScope::Node,
            ItemKind::Raw => SlotScope::Raw,
        }
    }

    /// Every item kind, in canonical order. Keeps the closed `item` vocabulary
    /// in one place, shared by the predicate registry and the language-def
    /// parser (which parses one `## <Item>` section per kind).
    pub fn all() -> [ItemKind; 8] {
        [
            ItemKind::Function,
            ItemKind::Struct,
            ItemKind::Enum,
            ItemKind::TypeDef,
            ItemKind::Const,
            ItemKind::Use,
            ItemKind::Tree,
            ItemKind::Raw,
        ]
    }
}

/// A function definition.
///
/// The minimal slice supports a name, no parameters, a single return type, and
/// a body. Parameters and richer bodies are added in later slices.
///
/// Structural equality **ignores** the `meta` field (see [`Meta`]).
#[derive(Debug, Clone)]
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
    /// Engine-transparent metadata (see [`Meta`]); default empty. A layer that
    /// lowers an anonymous function/closure to a hoisted `fn` tags it here
    /// (e.g. `origin=anon_fn` or `origin=lambda`) so a language definition can
    /// reconstruct the idiomatic inline form.
    pub meta: Meta,
}

impl PartialEq for Function {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.visibility == other.visibility
            && self.modifiers == other.modifiers
            && self.params == other.params
            && self.return_type == other.return_type
            && self.body == other.body
    }
}

impl Eq for Function {}

/// A function parameter: a name and a type.
///
/// Structural equality **ignores** the `meta` field (see [`Meta`]).
#[derive(Debug, Clone)]
pub struct Param {
    /// The parameter's identifier.
    pub name: String,
    /// The parameter's declared type.
    pub ty: Type,
    /// Engine-transparent metadata (see [`Meta`]); default empty.
    pub meta: Meta,
}

impl PartialEq for Param {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.ty == other.ty
    }
}

impl Eq for Param {}

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

/// A type-level attribute applied to a `struct` or `enum`.
///
/// This is the type-level analog of [`Modifier`]: the kernel reserves the
/// *superset* of behavioral capabilities a target may automatically derive for
/// an aggregate type (equality, ordering, hashing, cloning, a debug rendering,
/// a default value, iteration). Every target realizes each attribute in its own
/// way — a language definition maps each one to its target spelling (Rust folds
/// them into a single `#[derive(…)]` line), realizes it inherently (emitting no
/// text because the target provides the behavior for free), or forbids it (a
/// target that cannot realize the attribute as a type-level construct). Like a
/// callable modifier, a type attribute is semantic metadata first and emitted
/// text second — it stays attached to the IR node regardless of a target's
/// spelling.
///
/// The realization outcomes mirror the three ways a target answers a modifier:
/// - **realize** — emit the target's derive/annotation spelling (Rust
///   `#[derive(Debug)]`);
/// - **inherent** — emit nothing because the target provides the behavior with
///   no declaration (the metadata-first principle in action);
/// - **forbid** — the target cannot realize the attribute as a type attribute,
///   so a type carrying it cannot target that language (the `forbid` sentinel
///   surfaces a [`crate::error::EmitError::ForbiddenConstruct`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypeAttribute {
    /// Human-readable / diagnostic rendering (e.g. Rust `Debug`, Python
    /// `__repr__`, Java `toString`).
    Displayable,
    /// Value equality (e.g. Rust `PartialEq`/`Eq`, Python `__eq__`).
    Equatable,
    /// Total ordering / comparison (e.g. Rust `Ord`, Python `__lt__`, Java
    /// `Comparable`).
    Comparable,
    /// Hashability (e.g. Rust `Hash`, Python `__hash__`).
    Hashable,
    /// Deep duplication (e.g. Rust `Clone`, Python `__copy__`).
    Cloneable,
    /// Cheap value copy (e.g. Rust `Copy`; inherent in many targets).
    Copyable,
    /// A default / zero value (e.g. Rust `Default`, Go zero value).
    HasDefault,
    /// Iterability over the aggregate's elements. Many targets have no single
    /// type-level derive for this, so it is often `forbid`den or realized by a
    /// layer; it is retained as metadata regardless.
    Iterable,
}

impl TypeAttribute {
    /// The canonical Lamina spelling of this type attribute.
    pub fn as_str(self) -> &'static str {
        match self {
            TypeAttribute::Displayable => "displayable",
            TypeAttribute::Equatable => "equatable",
            TypeAttribute::Comparable => "comparable",
            TypeAttribute::Hashable => "hashable",
            TypeAttribute::Cloneable => "cloneable",
            TypeAttribute::Copyable => "copyable",
            TypeAttribute::HasDefault => "hasdefault",
            TypeAttribute::Iterable => "iterable",
        }
    }

    /// Resolves a type attribute from its canonical spelling.
    pub fn from_name(name: &str) -> Option<TypeAttribute> {
        match name {
            "displayable" => Some(TypeAttribute::Displayable),
            "equatable" => Some(TypeAttribute::Equatable),
            "comparable" => Some(TypeAttribute::Comparable),
            "hashable" => Some(TypeAttribute::Hashable),
            "cloneable" => Some(TypeAttribute::Cloneable),
            "copyable" => Some(TypeAttribute::Copyable),
            "hasdefault" => Some(TypeAttribute::HasDefault),
            "iterable" => Some(TypeAttribute::Iterable),
            _ => None,
        }
    }

    /// All type attributes in the reserved kernel superset, in canonical order.
    pub fn all() -> [TypeAttribute; 8] {
        [
            TypeAttribute::Displayable,
            TypeAttribute::Equatable,
            TypeAttribute::Comparable,
            TypeAttribute::Hashable,
            TypeAttribute::Cloneable,
            TypeAttribute::Copyable,
            TypeAttribute::HasDefault,
            TypeAttribute::Iterable,
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
    /// A fixed **array type** (kernel `array`): an element type plus an optional
    /// length. Rendering is target-specific (Rust `[T; N]` when a length is
    /// given, `[T]` when it is not; TypeScript `T[]`) and supplied by the
    /// language definition's `### array` type slot via the `elem` sub-slot (the
    /// element type) and the `len` sub-slot (the textual length, guarded by the
    /// `has_len` fact). Gated by the `### array` slot itself: a target with no
    /// array type forbids that slot.
    ///
    /// The length is kept **textual** (like an integer literal) so width /
    /// const-expression concerns stay a target matter. Richer/growable
    /// collections (lists, maps, sets) are LAYER concerns, not kernel — only a
    /// fixed array lives here.
    Array {
        /// The element type.
        elem: Box<Type>,
        /// The optional array length, preserved textually. `None` renders the
        /// unsized/slice form.
        len: Option<String>,
    },
}

impl Type {
    /// This type's engine-transparent [`Meta`] (see [`Meta`]).
    ///
    /// Metadata is exposed uniformly for every construct so the `has_meta` /
    /// `meta.<key>` facts and the `{meta.<key>}` slot are answerable in type
    /// scope. No layer in the current kernel tags a bare [`Type`] node (Part 3
    /// anonymous forms tag functions, fields, structs, and expressions, not
    /// types), so this is always the empty map today; it exists so the channel
    /// is present at every scope and a future layer that tags a type has a
    /// place to read it without a breaking change.
    pub fn meta(&self) -> &Meta {
        // A shared empty map: types carry no metadata in the current kernel.
        static EMPTY: std::sync::OnceLock<Meta> = std::sync::OnceLock::new();
        EMPTY.get_or_init(Meta::new)
    }

    /// Classifies this type into its closed [`TypeClass`] category.
    ///
    /// This is a **total**, engine-computed classification over the frozen
    /// [`Type`]/[`Primitive`] set: every primitive and every `Type` variant maps
    /// to exactly one class, checked by an exhaustive `match` with no catch-all
    /// (so a future primitive cannot be silently misclassified — adding one is a
    /// compile error until it is mapped here). It backs the closed
    /// `type is <class>` predicate fact, letting a language definition perform
    /// **type-directed dispatch** (choosing a `printf` specifier, a hash step, a
    /// serializer, …) from a small, closed vocabulary rather than a scripting
    /// surface.
    ///
    /// The classification is purely local to this node: a compound type
    /// (`Pointer`/`FnPtr`/`Array`) classifies by its outermost shape, not its
    /// element/pointee — the def recurses through the element's own `type` slot
    /// (and its own `type is <class>` fact) when it needs to.
    pub fn class(&self) -> TypeClass {
        match self {
            Type::Primitive(p) => p.class(),
            Type::Named(_) => TypeClass::Named,
            Type::Pointer(_) => TypeClass::Ptr,
            Type::FnPtr { .. } => TypeClass::FnPtr,
            Type::Array { .. } => TypeClass::Array,
        }
    }
}

/// A **closed** classification of a [`Type`] into a small category vocabulary,
/// for type-directed dispatch via the `type is <class>` predicate fact.
///
/// This mirrors the closed-dispatch pattern of
/// [`StmtKind`](crate::predicate::StmtKind) /
/// [`ExprKind`](crate::predicate::ExprKind): a fixed set of categories with an
/// [`as_str`](TypeClass::as_str) canonical spelling and an [`all`](TypeClass::all)
/// enumerator, so the engine (not the definition) owns the vocabulary. A
/// language definition may only *branch* on a class (`type is signed_int ->
/// "%d"`); it can neither invent a class nor inspect a type further — keeping the
/// engine dumb and the fact non-scripting.
///
/// [`Type::class`] maps every [`Type`] variant and every frozen [`Primitive`] to
/// exactly one of these classes with an exhaustive `match`; see that method for
/// the mapping rationale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypeClass {
    /// A signed integer primitive (`i8`/`i16`/`i32`/`i64`/`i128`/`isize`).
    SignedInt,
    /// An unsigned integer primitive
    /// (`u8`/`u16`/`u32`/`u64`/`u128`/`usize`, and `byte` — an octet is an
    /// unsigned 8-bit integer in value terms).
    UnsignedInt,
    /// A floating-point primitive (`f16`/`bf16`/`f32`/`f64`/`f128`).
    Float,
    /// The boolean primitive (`bool`).
    Bool,
    /// The Unicode-scalar character primitive (`char`).
    Char,
    /// The text-string primitive (`str`).
    String,
    /// The byte-buffer primitive (`bytes`) — a sequence of octets, distinct from
    /// a single `byte` and from a numeric integer.
    Bytes,
    /// The unit / no-value primitive (`void`).
    Unit,
    /// The uninhabited / never-returns primitive (`never`).
    Never,
    /// A raw pointer: the `ptr` primitive OR a [`Type::Pointer`].
    Ptr,
    /// A function pointer: the `fnptr` primitive OR a [`Type::FnPtr`].
    FnPtr,
    /// A reference to a user-defined type ([`Type::Named`] — a
    /// `struct`/`enum`/`typedef`).
    Named,
    /// A fixed array type ([`Type::Array`]).
    Array,
}

impl TypeClass {
    /// The canonical `type is <class>` value spelling for this class.
    pub fn as_str(self) -> &'static str {
        match self {
            TypeClass::SignedInt => "signed_int",
            TypeClass::UnsignedInt => "unsigned_int",
            TypeClass::Float => "float",
            TypeClass::Bool => "bool",
            TypeClass::Char => "char",
            TypeClass::String => "string",
            TypeClass::Bytes => "bytes",
            TypeClass::Unit => "unit",
            TypeClass::Never => "never",
            TypeClass::Ptr => "ptr",
            TypeClass::FnPtr => "fnptr",
            TypeClass::Named => "named",
            TypeClass::Array => "array",
        }
    }

    /// Resolves a type class from its canonical spelling. Returns `None` if
    /// `name` is not a known class.
    pub fn from_name(name: &str) -> Option<TypeClass> {
        TypeClass::all().into_iter().find(|c| c.as_str() == name)
    }

    /// The complete closed set of type classes, in canonical order.
    pub fn all() -> [TypeClass; 13] {
        [
            TypeClass::SignedInt,
            TypeClass::UnsignedInt,
            TypeClass::Float,
            TypeClass::Bool,
            TypeClass::Char,
            TypeClass::String,
            TypeClass::Bytes,
            TypeClass::Unit,
            TypeClass::Never,
            TypeClass::Ptr,
            TypeClass::FnPtr,
            TypeClass::Named,
            TypeClass::Array,
        ]
    }
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
        Primitive::all().into_iter().find(|p| p.as_str() == name)
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

    /// Classifies this primitive into its closed [`TypeClass`] category.
    ///
    /// The mapping is **total and exhaustive** over the frozen 26-primitive set
    /// (no catch-all), so adding a primitive is a compile error until it is
    /// mapped here. Rationale for the non-obvious groupings:
    ///
    /// - The width variants collapse into `signed_int` / `unsigned_int` /
    ///   `float`: a type-directed def cares about the *category* (which `printf`
    ///   specifier, which comparison), not the exact width, and the width is
    ///   already carried by the capability-matrix spelling of the `{type}` slot.
    /// - `byte` classifies as `unsigned_int`: an octet is an unsigned 8-bit
    ///   integer in value terms (C maps it to `uint8_t`), so it prints/compares
    ///   like one. `bytes` (a *buffer* of octets) is its own `bytes` class —
    ///   it is a sequence, not a scalar number.
    /// - `ptr`/`fnptr` classify alongside their compound [`Type`] forms
    ///   ([`Type::Pointer`]/[`Type::FnPtr`]) so a def branches on `ptr`/`fnptr`
    ///   uniformly whether the type is the bare primitive or a typed compound.
    pub fn class(self) -> TypeClass {
        match self {
            Primitive::I8
            | Primitive::I16
            | Primitive::I32
            | Primitive::I64
            | Primitive::I128
            | Primitive::Isize => TypeClass::SignedInt,
            Primitive::U8
            | Primitive::U16
            | Primitive::U32
            | Primitive::U64
            | Primitive::U128
            | Primitive::Usize
            | Primitive::Byte => TypeClass::UnsignedInt,
            Primitive::F16
            | Primitive::Bf16
            | Primitive::F32
            | Primitive::F64
            | Primitive::F128 => TypeClass::Float,
            Primitive::Bool => TypeClass::Bool,
            Primitive::Char => TypeClass::Char,
            Primitive::Str => TypeClass::String,
            Primitive::Bytes => TypeClass::Bytes,
            Primitive::Void => TypeClass::Unit,
            Primitive::Never => TypeClass::Never,
            Primitive::Ptr => TypeClass::Ptr,
            Primitive::Fnptr => TypeClass::FnPtr,
        }
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
///
/// Structural equality **ignores** any metadata a variant carries (see
/// [`Meta`]); it is hand-implemented to compare structure only (the
/// [`Statement::Raw`] escape hatch carries a metadata channel like every other
/// node, so a derived `PartialEq` would wrongly compare it).
#[derive(Debug, Clone)]
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
    /// An assignment (`target = value;`). `target` is an *lvalue* — a place an
    /// assignment can name — restricted to [`Expr::Ref`], [`Expr::Field`], or
    /// [`Expr::Index`]. Assigning to a literal or an operator expression is
    /// meaningless, so [`Statement::assign`] validates the target at
    /// construction. Compound assignment (`+=`) is NOT a distinct kernel node:
    /// it is a language-definition *idiom* recognized from an `Assign` whose
    /// `value` is a binary whose left operand equals the `target` (see the
    /// one-level structural predicates in the language-definition format).
    Assign {
        /// The assignment target: an lvalue [`Expr`] (`Ref`/`Field`/`Index`).
        target: Expr,
        /// The value assigned to the target.
        value: Expr,
    },
    /// An expression-statement (e.g. a bare function call `f();`).
    Expr(Expr),
    /// A **raw / verbatim statement**: a string of literal target code the
    /// engine emits UNCHANGED at the slot position (the ultimate layer escape
    /// hatch).
    ///
    /// Like [`Expr::Raw`], a raw statement is NOT target-keyed: it exists in
    /// the AST only when a layer lowered it *for the current target*, so the
    /// engine passes the string through with no target check, capability
    /// gating, or error path. The verbatim string is exposed to the target's
    /// `### statement` `stmt is raw` row via the `value` scalar slot; a
    /// multi-line raw statement in an indented body is re-indented by the
    /// renderer's ordinary column-derived continuation-line indentation, exactly
    /// as any other multi-line rendered fragment (see the emitter's indentation
    /// docs). Structural equality compares the string and ignores `meta`.
    Raw {
        /// The verbatim target-code fragment, emitted unchanged.
        code: String,
        /// Engine-transparent metadata (see [`Meta`]); default empty. A raw
        /// statement is raiseable like any other node.
        meta: Meta,
    },
}

/// Structural equality of statements **ignores metadata** (see [`Meta`]): two
/// statements are equal iff their structure matches. It is hand-implemented
/// because [`Statement::Raw`] carries a metadata channel that must not
/// participate in equality (consistent with the rest of the AST). Every other
/// variant compares its structural fields as a derived `PartialEq` would.
impl PartialEq for Statement {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Statement::Block(a), Statement::Block(b)) => a == b,
            (
                Statement::Let {
                    name: an,
                    ty: at,
                    value: av,
                },
                Statement::Let {
                    name: bn,
                    ty: bt,
                    value: bv,
                },
            ) => an == bn && at == bt && av == bv,
            (Statement::Return(a), Statement::Return(b)) => a == b,
            (
                Statement::If {
                    cond: ac,
                    then_block: at,
                    else_block: ae,
                },
                Statement::If {
                    cond: bc,
                    then_block: bt,
                    else_block: be,
                },
            ) => ac == bc && at == bt && ae == be,
            (
                Statement::While { cond: ac, body: ab },
                Statement::While { cond: bc, body: bb },
            ) => ac == bc && ab == bb,
            (
                Statement::For {
                    init: ai,
                    cond: ac,
                    step: as_,
                    body: ab,
                },
                Statement::For {
                    init: bi,
                    cond: bc,
                    step: bs,
                    body: bb,
                },
            ) => ai == bi && ac == bc && as_ == bs && ab == bb,
            (
                Statement::ForEach {
                    binding: ab,
                    iterable: ai,
                    body: abd,
                },
                Statement::ForEach {
                    binding: bb,
                    iterable: bi,
                    body: bbd,
                },
            ) => ab == bb && ai == bi && abd == bbd,
            (
                Statement::Switch {
                    scrutinee: asc,
                    cases: ac,
                    default: ad,
                },
                Statement::Switch {
                    scrutinee: bsc,
                    cases: bc,
                    default: bd,
                },
            ) => asc == bsc && ac == bc && ad == bd,
            (Statement::Break, Statement::Break) => true,
            (Statement::Continue, Statement::Continue) => true,
            (
                Statement::Assign {
                    target: at,
                    value: av,
                },
                Statement::Assign {
                    target: bt,
                    value: bv,
                },
            ) => at == bt && av == bv,
            (Statement::Expr(a), Statement::Expr(b)) => a == b,
            // A raw statement's structural identity is its verbatim string;
            // metadata is ignored (consistent with every other node).
            (Statement::Raw { code: a, .. }, Statement::Raw { code: b, .. }) => a == b,
            _ => false,
        }
    }
}

impl Eq for Statement {}

/// Returns `true` if `expr` is a valid *lvalue* — a place an assignment can
/// name. The kernel lvalues are a variable [`Expr::Ref`], a field access
/// [`Expr::Field`], and an index [`Expr::Index`]; everything else (a literal,
/// a call, an operator expression, a cast, a struct literal) denotes a value,
/// not a place, so it cannot be assigned to.
pub fn is_lvalue(expr: &Expr) -> bool {
    matches!(expr, Expr::Ref(_) | Expr::Field { .. } | Expr::Index { .. })
}

impl Statement {
    /// Builds a [`Statement::Assign`], validating that `target` is an lvalue
    /// (see [`is_lvalue`]).
    ///
    /// # Errors
    ///
    /// Returns [`AstError::NotAnLvalue`] if `target` is not a `Ref`, `Field`,
    /// or `Index`.
    pub fn assign(target: Expr, value: Expr) -> Result<Statement, AstError> {
        if is_lvalue(&target) {
            Ok(Statement::Assign { target, value })
        } else {
            Err(AstError::NotAnLvalue {
                kind: target.kind().as_str(),
            })
        }
    }
}

/// An error building an AST node whose invariants are checked at construction.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AstError {
    /// An assignment target was not an lvalue (`Ref`/`Field`/`Index`).
    #[error("assignment target must be an lvalue (ref/field/index), got {kind:?}")]
    NotAnLvalue {
        /// The offending target expression's kind spelling.
        kind: &'static str,
    },
}

/// One `case` of a [`Statement::Switch`]: a matched value and its body.
///
/// Structural equality **ignores** the `meta` field (see [`Meta`]).
#[derive(Debug, Clone)]
pub struct SwitchCase {
    /// The value this case matches against the scrutinee.
    pub value: Expr,
    /// The statements run when the case matches.
    pub body: Vec<Statement>,
    /// Engine-transparent metadata (see [`Meta`]); default empty.
    pub meta: Meta,
}

impl PartialEq for SwitchCase {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value && self.body == other.body
    }
}

impl Eq for SwitchCase {}

impl Statement {
    /// This statement's engine-transparent [`Meta`] (see [`Meta`]).
    ///
    /// Metadata is exposed uniformly so the `has_meta` / `meta.<key>` facts and
    /// the `{meta.<key>}` slot are answerable in statement scope. The only
    /// statement variant that carries an inline metadata channel is
    /// [`Statement::Raw`] (a raw statement is raiseable like any other node);
    /// every other kind exposes the empty map — no layer in the current kernel
    /// tags them (the Part 3 anonymous forms tag functions, fields, structs,
    /// and expressions — see [`Function::meta`], [`Field::meta`],
    /// [`Item::meta`], and [`Expr::meta`] — not statements). The channel is
    /// present at every scope so a future layer that tags a statement has a
    /// place to read it without a breaking change.
    pub fn meta(&self) -> &Meta {
        match self {
            Statement::Raw { meta, .. } => meta,
            _ => {
                static EMPTY: std::sync::OnceLock<Meta> = std::sync::OnceLock::new();
                EMPTY.get_or_init(Meta::new)
            }
        }
    }
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
            Statement::Assign { .. } => StatementKind::Assign,
            Statement::Expr(_) => StatementKind::Expr,
            Statement::Raw { .. } => StatementKind::Raw,
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
    /// An assignment.
    Assign,
    /// An expression-statement.
    Expr,
    /// A raw / verbatim statement (the layer escape hatch).
    Raw,
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
            StatementKind::Assign => "assign",
            StatementKind::Expr => "expr",
            StatementKind::Raw => "raw",
        }
    }

    /// Every statement kind, in canonical order. Keeps the closed `stmt`
    /// vocabulary in one place, shared by the predicate registry.
    pub fn all() -> [StatementKind; 13] {
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
            StatementKind::Assign,
            StatementKind::Expr,
            StatementKind::Raw,
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
///
/// Structural equality **ignores** any metadata a variant carries (see
/// [`Meta`]); it is hand-implemented to compare structure only.
#[derive(Debug, Clone)]
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
    /// A cast of a value to a target type (kernel spelling `value as Type`).
    ///
    /// Casts are REQUIRED by the capability model: a `widen` records a width
    /// diagnostic so a later narrow (storing back into a smaller type) must be
    /// *explicit* — and an explicit narrow is exactly a `Cast`. The target
    /// spells it per its `### cast` slot (Rust `x as i64`, TypeScript
    /// `x as number`, a C-family `(T)x`); a target that cannot express the cast
    /// forbids that slot.
    Cast {
        /// The value being cast.
        value: Box<Expr>,
        /// The target type.
        ty: Type,
    },
    /// An aggregate (struct) construction literal
    /// (`TypeName { field: value, … }`).
    ///
    /// This is the kernel *construction* expression: it names an aggregate type
    /// and initializes its fields. The target spells it per its `### struct_lit`
    /// slot (Rust `Account { balance: 0 }`, TypeScript `{ balance: 0 }` or
    /// `new Account(…)` — the definition's choice). Fields render as a sequence
    /// slot (item slot `field_init`, with `first`/`last` loop facts), mirroring
    /// the existing collection mechanism. A bare function name stored into a
    /// `fnptr`-typed field is a function-pointer value (see [`Expr::Ref`]).
    StructLit {
        /// The aggregate type being constructed.
        type_name: String,
        /// The field initializers, in order (possibly empty).
        fields: Vec<FieldInit>,
        /// Engine-transparent metadata (see [`Meta`]); default empty. A layer
        /// that lowers an anonymous class (or a closure's capture environment)
        /// tags the construction site here (e.g. `origin=anon_class` or
        /// `role=closure_env`) so a language definition can reconstruct the
        /// idiomatic inline object/closure form.
        meta: Meta,
    },
    /// An **array literal** (`[a, b, c]`): an ordered list of element
    /// expressions.
    ///
    /// This is the kernel array *construction* expression, the value-level
    /// companion to [`Type::Array`]. The target spells it per its `### expr`
    /// `array` row (Rust / TypeScript both `[a, b, c]`); the elements render as
    /// a sequence slot (item slot `array_elem`, with `first`/`last` loop facts),
    /// mirroring the existing collection mechanism — a non-first element renders
    /// its own `, ` separator via the `!first` idiom. Indexing an array uses the
    /// existing [`Expr::Index`].
    ArrayLit {
        /// The element expressions, in order (possibly empty).
        elems: Vec<Expr>,
        /// Engine-transparent metadata (see [`Meta`]); default empty.
        meta: Meta,
    },
    /// A **declarative tree node** (the tree core): a named node with
    /// attributes and children.
    ///
    /// This is the universal substrate of every document and structured-data
    /// format — an HTML element, a CSS rule, a JSON object, a YAML/TOML table
    /// are all "a named node with attributes and children." The kernel keeps
    /// the vocabulary generic: `name` is an opaque string whose meaning is a
    /// language-definition / layer concern (an `html` layer defines `div`; a
    /// JSON target maps the same node to an object), so NO format-specific
    /// keyword lives in the kernel.
    ///
    /// A node is a first-class *expression*, so the imperative core can hold,
    /// return, and build one (a `fn` may return a `Node`; a child or an
    /// attribute value may be an arbitrary expression, enabling JSX-like
    /// interpolation `<div>{name}</div>`). Children are arbitrary expressions —
    /// a child may be another `Node`, a `Text`, or any other [`Expr`].
    Node {
        /// The node's name (generic — the language def / layer gives it
        /// meaning).
        name: String,
        /// The node's attributes, in order (possibly empty).
        attrs: Vec<Attr>,
        /// The node's children, in order (possibly empty). Each child is an
        /// arbitrary expression (another node, a text node, or an interpolated
        /// value).
        children: Vec<Expr>,
        /// Engine-transparent metadata (see [`Meta`]); default empty. A layer
        /// may tag a node (e.g. `void=true` for an HTML void element) so a
        /// language definition can branch on it.
        meta: Meta,
    },
    /// **Literal text content** in a tree (the tree core). The inner expression
    /// is typically a [`Expr::StringLiteral`] but may be any expression
    /// (interpolation), so a text node can carry an interpolated value the same
    /// way a child can. Modelling `text` as an explicit node (rather than
    /// merely allowing a bare literal child) lets a target treat text
    /// distinctly from element children — e.g. HTML escapes text content but
    /// not element markup, and JSON quotes a string scalar.
    Text(Box<Expr>),
    /// A **raw / verbatim expression fragment**: a string of literal target
    /// code the engine emits UNCHANGED at the slot position (the ultimate
    /// layer escape hatch).
    ///
    /// A raw node lets a *layer* lower an expression that Lamina's kernel
    /// vocabulary plus the language definition cannot otherwise express,
    /// guaranteeing the layer can always produce the exact target code. It is
    /// deliberately **not** target-keyed: a layer lowers differently per
    /// target, so a raw node only ever exists in the AST when the layer lowered
    /// *for the current target* — by the time the engine sees it, it is always
    /// correct target code by construction. The engine therefore performs NO
    /// target check, variant selection, capability gating, or error path; it
    /// simply passes the string through.
    ///
    /// The verbatim string is exposed to the target's `### expr` `expr is raw`
    /// row via the `value` scalar slot. Structural equality compares the string
    /// and (like every other node) **ignores** the `meta` field.
    Raw {
        /// The verbatim target-code fragment, emitted unchanged.
        code: String,
        /// Engine-transparent metadata (see [`Meta`]); default empty. A raw
        /// node is raiseable like any other node — it carries the standard
        /// metadata channel with no special-casing.
        meta: Meta,
    },
    /// A **lambda**: a first-class anonymous function value — the kernel's ONE
    /// functional-core primitive.
    ///
    /// A lambda is essentially an anonymous [`Function`] value: it reuses the
    /// same [`Param`] and statement-body machinery. It is the single
    /// irreducible functional construct the imperative kernel was missing —
    /// `map`/`fold`/`filter`, ranges, list comprehensions, and let-expressions
    /// all REDUCE to *lambda + recursion + application + collections*, so they
    /// are LIBRARY concerns rendered per-target at the call site, NOT kernel.
    /// Adding this one primitive makes the kernel dual-paradigm (imperative +
    /// functional).
    ///
    /// The `body` is a statement **block** (the most general form), NOT
    /// expression-only: lambda body shape varies by target
    /// (Python/Haskell are expression-only; JS/Rust/Swift/Kotlin allow
    /// statement blocks), so the kernel holds the general form and a
    /// single-expression lambda is simply a one-statement body (a `return` or
    /// expression statement) that a def may render concisely.
    ///
    /// **Capture is the target's concern** — the kernel adds NO capture
    /// analysis. Closure-capable targets (Rust, TS/JS, Haskell, Swift, Kotlin)
    /// render this node DIRECTLY as their native closure. Expression-only
    /// targets (Python) render a single-expression lambda directly and HOIST a
    /// multi-statement one to a named `def`; lambda-less targets (C) hoist to a
    /// named function + fnptr — both via the existing `origin=lambda` metadata /
    /// `fresh_name` reconstruction path, of which `Expr::Lambda` is now the
    /// PRIMARY kernel representation.
    ///
    /// Structural equality **ignores** the `meta` field (see [`Meta`]).
    Lambda {
        /// The lambda's parameters, in order (possibly empty). Reuses the
        /// existing [`Param`] (name + type).
        params: Vec<Param>,
        /// The optional declared return type. Inference targets (where the
        /// return type is elided) omit it; the `has_ret_type` fact guards its
        /// rendering.
        return_type: Option<Type>,
        /// The lambda's body: a statement block (the general form). A
        /// single-expression lambda is a one-statement body.
        body: Vec<Statement>,
        /// Engine-transparent metadata (see [`Meta`]); default empty. The
        /// existing lambda-reconstruction path tags a hoisted anonymous
        /// function here (e.g. `origin=lambda`) so a lambda-less / expression-
        /// only target can reconstruct the idiomatic form.
        meta: Meta,
    },
}

/// An **attribute** on a tree [`Expr::Node`]: a name and a value expression.
///
/// The value reuses the imperative core's literal/expression nodes (per the
/// tree-core decision to share one value system), so an attribute value may be
/// a [`Expr::StringLiteral`], an [`Expr::IntLiteral`], or an arbitrary
/// expression (e.g. an interpolated `{someVar}`).
///
/// Structural equality **ignores** the `meta` field (see [`Meta`]).
#[derive(Debug, Clone)]
pub struct Attr {
    /// The attribute's name (generic — the language def / layer gives it
    /// meaning).
    pub name: String,
    /// The attribute's value expression (reuses the imperative literals).
    pub value: Expr,
    /// Engine-transparent metadata (see [`Meta`]); default empty.
    pub meta: Meta,
}

impl PartialEq for Attr {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.value == other.value
    }
}

impl Eq for Attr {}

/// One field initializer of a [`Expr::StructLit`]: a field name and its value.
///
/// Structural equality **ignores** the `meta` field (see [`Meta`]).
#[derive(Debug, Clone)]
pub struct FieldInit {
    /// The field's name.
    pub name: String,
    /// The value assigned to the field.
    pub value: Expr,
    /// Engine-transparent metadata (see [`Meta`]); default empty.
    pub meta: Meta,
}

impl PartialEq for FieldInit {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.value == other.value
    }
}

impl Eq for FieldInit {}

/// Structural equality of expressions **ignores metadata** (see [`Meta`]): two
/// expressions are equal iff their structure matches. This preserves the `eq`
/// structural predicate (e.g. `target eq value.lhs` for the compound-assign
/// idiom) regardless of what a layer tagged.
impl PartialEq for Expr {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Expr::IntLiteral(a), Expr::IntLiteral(b))
            | (Expr::FloatLiteral(a), Expr::FloatLiteral(b))
            | (Expr::StringLiteral(a), Expr::StringLiteral(b))
            | (Expr::CharLiteral(a), Expr::CharLiteral(b))
            | (Expr::Ref(a), Expr::Ref(b)) => a == b,
            (Expr::BoolLiteral(a), Expr::BoolLiteral(b)) => a == b,
            (Expr::NullLiteral, Expr::NullLiteral) => true,
            (Expr::Field { obj: ao, field: af }, Expr::Field { obj: bo, field: bf }) => {
                ao == bo && af == bf
            }
            (Expr::Index { obj: ao, index: ai }, Expr::Index { obj: bo, index: bi }) => {
                ao == bo && ai == bi
            }
            (
                Expr::Call {
                    callee: ac,
                    args: aa,
                },
                Expr::Call {
                    callee: bc,
                    args: ba,
                },
            ) => ac == bc && aa == ba,
            (
                Expr::Unary {
                    op: aop,
                    operand: ao,
                },
                Expr::Unary {
                    op: bop,
                    operand: bo,
                },
            ) => aop == bop && ao == bo,
            (
                Expr::Binary {
                    op: aop,
                    lhs: al,
                    rhs: ar,
                },
                Expr::Binary {
                    op: bop,
                    lhs: bl,
                    rhs: br,
                },
            ) => aop == bop && al == bl && ar == br,
            (Expr::Cast { value: av, ty: at }, Expr::Cast { value: bv, ty: bt }) => {
                av == bv && at == bt
            }
            (
                Expr::StructLit {
                    type_name: at,
                    fields: af,
                    ..
                },
                Expr::StructLit {
                    type_name: bt,
                    fields: bf,
                    ..
                },
            ) => at == bt && af == bf,
            (
                Expr::ArrayLit { elems: ae, .. },
                Expr::ArrayLit { elems: be, .. },
            ) => ae == be,
            (
                Expr::Node {
                    name: an,
                    attrs: aa,
                    children: ac,
                    ..
                },
                Expr::Node {
                    name: bn,
                    attrs: ba,
                    children: bc,
                    ..
                },
            ) => an == bn && aa == ba && ac == bc,
            (Expr::Text(a), Expr::Text(b)) => a == b,
            // A raw node's structural identity is its verbatim string; metadata
            // is ignored (consistent with every other node).
            (Expr::Raw { code: a, .. }, Expr::Raw { code: b, .. }) => a == b,
            // A lambda's structural identity is its params, return type, and
            // body; metadata is ignored (consistent with every other node).
            (
                Expr::Lambda {
                    params: ap,
                    return_type: ar,
                    body: ab,
                    ..
                },
                Expr::Lambda {
                    params: bp,
                    return_type: br,
                    body: bb,
                    ..
                },
            ) => ap == bp && ar == br && ab == bb,
            _ => false,
        }
    }
}

impl Eq for Expr {}

impl Expr {
    /// This expression's engine-transparent [`Meta`] (see [`Meta`]).
    ///
    /// Only [`Expr::StructLit`] carries inline metadata today (the Part 3
    /// anonymous-class / closure-environment use site); every other expression
    /// kind reads its intent off the *resolved* definition it references — e.g.
    /// a `fnptr` [`Expr::Ref`] resolves to a hoisted [`Function`] and reads
    /// [`Function::meta`]. Those kinds therefore expose the empty map here so
    /// the `has_meta` / `meta.<key>` facts and `{meta.<key>}` slot are
    /// answerable uniformly in expression scope.
    pub fn meta(&self) -> &Meta {
        match self {
            Expr::StructLit { meta, .. } => meta,
            Expr::Node { meta, .. } => meta,
            Expr::ArrayLit { meta, .. } => meta,
            Expr::Raw { meta, .. } => meta,
            Expr::Lambda { meta, .. } => meta,
            _ => {
                static EMPTY: std::sync::OnceLock<Meta> = std::sync::OnceLock::new();
                EMPTY.get_or_init(Meta::new)
            }
        }
    }

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
            Expr::Cast { .. } => ExprKind::Cast,
            Expr::StructLit { .. } => ExprKind::StructLit,
            Expr::Node { .. } => ExprKind::Node,
            Expr::Text(_) => ExprKind::Text,
            Expr::ArrayLit { .. } => ExprKind::ArrayLit,
            Expr::Raw { .. } => ExprKind::Raw,
            Expr::Lambda { .. } => ExprKind::Lambda,
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
    /// A cast expression.
    Cast,
    /// A struct-literal (construction) expression.
    StructLit,
    /// A declarative tree node (the tree core).
    Node,
    /// A declarative tree text-content node (the tree core).
    Text,
    /// An array-literal (construction) expression.
    ArrayLit,
    /// A raw / verbatim expression fragment (the layer escape hatch).
    Raw,
    /// A lambda (anonymous function value) — the functional-core primitive.
    Lambda,
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
            ExprKind::Cast => "cast",
            ExprKind::StructLit => "struct_lit",
            ExprKind::Node => "node",
            ExprKind::Text => "text",
            ExprKind::ArrayLit => "array",
            ExprKind::Raw => "raw",
            ExprKind::Lambda => "lambda",
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
    /// Resolving slots of a single [`FieldInit`] element (one element of a
    /// [`Expr::StructLit`]'s `fields`). Exposes `name` (the field name) and
    /// `value` (the field's value expression); loop facts (`first`/`last`) let
    /// the item template supply its own separator.
    FieldInit,
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
    /// Resolving slots of a single type-attribute element (one element of a
    /// struct's or enum's `attributes`). Exposes `name` — the attribute's
    /// target spelling — so the item template maps each attribute to its target
    /// derive/annotation text; loop facts (`first`/`last`) let the item
    /// template supply its own separator (e.g. Rust's `, ` between derives).
    Attribute,
    /// Resolving slots of a single tuple-payload type element (one element of a
    /// tuple-style [`Variant`]'s `payload_types`). Exposes `type` (the rendered
    /// payload type); loop facts let the item template supply its own
    /// separator.
    PayloadType,
    /// Resolving slots of a [`Item::TypeDef`] declaration: its `name` and
    /// `target` (the aliased type, rendered as a scalar).
    TypeDef,
    /// Resolving slots of a [`Item::Const`] declaration: its `name`, `type`,
    /// and `value` (all scalar).
    Const,
    /// Resolving slots of a [`Item::Use`] declaration: its `path` (scalar).
    Use,
    /// Resolving slots of a single [`UseItem`] element (one element of a
    /// selective [`Item::Use`]'s `items`). Exposes `name` (the imported item's
    /// name) and, when aliased, `alias`; the `has_alias` fact guards the alias.
    /// Loop facts let the item template supply its own separator.
    UseItem,
    /// Resolving slots of a tree [`Expr::Node`]: its `name` (scalar), and its
    /// `attrs` / `children` sequences (which loop the `attr` / `child` item
    /// slots).
    Node,
    /// Resolving slots of a single [`Attr`] element (one element of a node's
    /// `attrs`). Exposes `name` (the attribute name) and `value` (the
    /// attribute's value expression, dispatched through the `### expr` table);
    /// loop facts (`first`/`last`) let the item template supply its own
    /// separator.
    Attr,
    /// Resolving slots of a single node child element (one element of a node's
    /// `children`). Its `value` sub-slot renders the child expression
    /// (dispatching through the `### expr` table); loop facts (`first`/`last`)
    /// let the item template supply its own separator.
    Child,
    /// Resolving slots of a single array-literal element (one element of an
    /// [`Expr::ArrayLit`]'s `elems`). Its `value` sub-slot renders the element
    /// expression (dispatching through the `### expr` table); loop facts
    /// (`first`/`last`) let the item template supply its own separator.
    ArrayElem,
    /// Resolving slots of a raw / verbatim top-level [`Item::Raw`]. Exposes a
    /// single `value` scalar leaf: the verbatim code string, emitted unchanged.
    /// (The `expr is raw` / `stmt is raw` forms reuse the [`SlotScope::Expr`] /
    /// [`SlotScope::Statement`] `value` slot, so no separate scope is needed for
    /// them.)
    Raw,
    /// Resolving slots of a [`Expr::Lambda`]: its `params` (a Sequence looping
    /// the shared `param` item slot, resolved in [`SlotScope::Param`]), its
    /// optional `ret_type` (a scalar, guarded by the `has_ret_type` fact), and
    /// its `body` (a Sequence of statements looping the recursive `statement`
    /// item slot, resolved in [`SlotScope::Statement`]). A closure-capable
    /// target renders these into its native closure syntax; capture is the
    /// target's concern.
    Lambda,
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
            // 0-based loop ordinal (see [`RenderContext::index`]).
            "index" => Some(SlotShape::Scalar),
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
        //
        // `init_clause` / `step_clause` are the *clause* (terminator-free) forms
        // of a `for` statement's `init` / `step`, rendered through the target's
        // `### stmt_clause` dispatch rather than `### statement`. A C-style
        // `for (init; cond; step)` header composes clauses joined by the header
        // syntax, so the init/step must NOT carry a statement terminator (the
        // header supplies its own `;` separators). Targets that desugar the
        // counted loop instead (e.g. Rust, which has no C-style `for`) use the
        // full-statement `init` / `step` slots, where the trailing terminator is
        // correct. All spelling stays in the definition; the engine only routes
        // to the `### stmt_clause` slot and appends nothing.
        SlotScope::Statement => match name {
            "name" => Some(SlotShape::Scalar),
            "binding" => Some(SlotShape::Scalar),
            "let_type" => Some(SlotShape::Scalar),
            "value" => Some(SlotShape::Scalar),
            "target" => Some(SlotShape::Scalar),
            "cond" => Some(SlotShape::Scalar),
            "iterable" => Some(SlotShape::Scalar),
            "scrutinee" => Some(SlotShape::Scalar),
            "else" => Some(SlotShape::Scalar),
            "init" => Some(SlotShape::Scalar),
            "step" => Some(SlotShape::Scalar),
            "init_clause" => Some(SlotShape::Scalar),
            "step_clause" => Some(SlotShape::Scalar),
            // One-level structural sub-part slots (Part 2). A dotted slot name
            // `value.rhs` / `value.lhs` / `value.op` references a DIRECT named
            // sub-part of the current statement's `value` sub-expression when
            // it is a binary, letting a language def render a compound-assign
            // idiom (`x += y` from `x = x + y`). One level only — deeper paths
            // are not bound. `value.op` renders the operator spelling; the
            // others render the rendered sub-expression.
            "value.lhs" | "value.rhs" | "value.op" => Some(SlotShape::Scalar),
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
            // 0-based loop ordinal (see [`RenderContext::index`]).
            "index" => Some(SlotShape::Scalar),
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
            // An array type's element type (scalar, dispatched through the type
            // machinery) and its optional textual length (scalar leaf, guarded
            // by the `has_len` fact).
            "elem" => Some(SlotShape::Scalar),
            "len" => Some(SlotShape::Scalar),
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
            "ty" => Some(SlotShape::Scalar),
            "type_name" => Some(SlotShape::Scalar),
            // A tree node's own name (scalar leaf). `value` (already bound
            // above) doubles as the tree text node's inner expression.
            "node_name" => Some(SlotShape::Scalar),
            "args" => Some(SlotShape::Sequence {
                item_slot: "expr_arg".to_string(),
                item_scope: SlotScope::ExprArg,
            }),
            "fields" => Some(SlotShape::Sequence {
                item_slot: "field_init".to_string(),
                item_scope: SlotScope::FieldInit,
            }),
            // An array literal's element expressions loop the `array_elem` item
            // slot.
            "elems" => Some(SlotShape::Sequence {
                item_slot: "array_elem".to_string(),
                item_scope: SlotScope::ArrayElem,
            }),
            // A tree node's attributes and children loop their own item slots.
            "attrs" => Some(SlotShape::Sequence {
                item_slot: "attr".to_string(),
                item_scope: SlotScope::Attr,
            }),
            "children" => Some(SlotShape::Sequence {
                item_slot: "child".to_string(),
                item_scope: SlotScope::Child,
            }),
            _ => None,
        },
        // A single call-argument element: `value` renders the argument
        // expression (dispatching through the `### expr` table).
        SlotScope::ExprArg => match name {
            "value" => Some(SlotShape::Scalar),
            // 0-based loop ordinal (see [`RenderContext::index`]).
            "index" => Some(SlotShape::Scalar),
            _ => None,
        },
        // A single struct-literal field initializer: `name` is the field name
        // (scalar leaf) and `value` renders the field's value expression
        // (dispatching through the `### expr` table).
        SlotScope::FieldInit => match name {
            "name" => Some(SlotShape::Scalar),
            "value" => Some(SlotShape::Scalar),
            // 0-based loop ordinal (see [`RenderContext::index`]).
            "index" => Some(SlotShape::Scalar),
            _ => None,
        },
        // A struct declaration: `name` is a scalar leaf; `fields` loops the
        // `field` item slot; `attributes` loops the `attribute` item slot.
        SlotScope::Struct => match name {
            "name" => Some(SlotShape::Scalar),
            "fields" => Some(SlotShape::Sequence {
                item_slot: "field".to_string(),
                item_scope: SlotScope::Field,
            }),
            "attributes" => Some(SlotShape::Sequence {
                item_slot: "attribute".to_string(),
                item_scope: SlotScope::Attribute,
            }),
            _ => None,
        },
        // A single struct-field element: `name` and `type` are scalar leaves
        // (`type` resolves the field's declared type).
        SlotScope::Field => match name {
            "name" => Some(SlotShape::Scalar),
            "type" => Some(SlotShape::Scalar),
            // 0-based loop ordinal (see [`RenderContext::index`]).
            "index" => Some(SlotShape::Scalar),
            _ => None,
        },
        // An enum declaration: `name` is a scalar leaf; `variants` loops the
        // `variant` item slot; `attributes` loops the `attribute` item slot.
        SlotScope::Enum => match name {
            "name" => Some(SlotShape::Scalar),
            "variants" => Some(SlotShape::Sequence {
                item_slot: "variant".to_string(),
                item_scope: SlotScope::Variant,
            }),
            "attributes" => Some(SlotShape::Sequence {
                item_slot: "attribute".to_string(),
                item_scope: SlotScope::Attribute,
            }),
            _ => None,
        },
        // A single enum-variant element: `name` is a scalar leaf. A payload-
        // bearing variant also exposes its payload as a sequence: a tuple
        // variant's `payload_types` loop the `payload_type` item slot; a struct
        // variant's `payload_fields` loop the `payload_field` item slot (which
        // resolves in [`SlotScope::Field`], reusing the struct-field `name`/
        // `type` sub-slots). Which sequence is meaningful depends on the
        // variant's shape (the `variant is tuple|struct` dispatch fact).
        SlotScope::Variant => match name {
            "name" => Some(SlotShape::Scalar),
            "payload_types" => Some(SlotShape::Sequence {
                item_slot: "payload_type".to_string(),
                item_scope: SlotScope::PayloadType,
            }),
            "payload_fields" => Some(SlotShape::Sequence {
                item_slot: "payload_field".to_string(),
                item_scope: SlotScope::Field,
            }),
            // 0-based loop ordinal (see [`RenderContext::index`]).
            "index" => Some(SlotShape::Scalar),
            _ => None,
        },
        // A single type-attribute element: `name` is the attribute's canonical
        // spelling (a scalar leaf). The item template maps each spelling to its
        // target derive/annotation text; loop facts supply the separator.
        SlotScope::Attribute => match name {
            "name" => Some(SlotShape::Scalar),
            // 0-based loop ordinal (see [`RenderContext::index`]).
            "index" => Some(SlotShape::Scalar),
            _ => None,
        },
        // A single tuple-payload type element: `type` is the rendered payload
        // type (dispatched through the type machinery).
        SlotScope::PayloadType => match name {
            "type" => Some(SlotShape::Scalar),
            // 0-based loop ordinal (see [`RenderContext::index`]) — the source
            // of a C tuple-payload tagged union's numbered members (`_0`, `_1`).
            "index" => Some(SlotShape::Scalar),
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
        // A `use` import: `path` is a scalar leaf rendered verbatim. A module
        // alias (`use path as p`) exposes `alias` (scalar leaf, guarded by the
        // `has_alias` fact); a selective import exposes `items` (a sequence
        // looping the `use_item` item slot, guarded by the `has_items` fact).
        SlotScope::Use => match name {
            "path" => Some(SlotShape::Scalar),
            "alias" => Some(SlotShape::Scalar),
            "items" => Some(SlotShape::Sequence {
                item_slot: "use_item".to_string(),
                item_scope: SlotScope::UseItem,
            }),
            _ => None,
        },
        // A single selectively-imported item: `name` is the imported name
        // (scalar leaf); `alias` is the optional local alias (scalar leaf,
        // guarded by the `has_alias` fact).
        SlotScope::UseItem => match name {
            "name" => Some(SlotShape::Scalar),
            "alias" => Some(SlotShape::Scalar),
            // 0-based loop ordinal (see [`RenderContext::index`]).
            "index" => Some(SlotShape::Scalar),
            _ => None,
        },
        // A tree node, when rendered as an item (top-level tree value). Its
        // `node_name` is a scalar leaf; `attrs`/`children` loop their item
        // slots. (When a node is rendered as an *expression* — the usual case —
        // these same slots are bound in [`SlotScope::Expr`] above.)
        SlotScope::Node => match name {
            "node_name" => Some(SlotShape::Scalar),
            "value" => Some(SlotShape::Scalar),
            "attrs" => Some(SlotShape::Sequence {
                item_slot: "attr".to_string(),
                item_scope: SlotScope::Attr,
            }),
            "children" => Some(SlotShape::Sequence {
                item_slot: "child".to_string(),
                item_scope: SlotScope::Child,
            }),
            _ => None,
        },
        // A single attribute element: `name` is the attribute name (scalar
        // leaf); `value` renders the attribute's value expression (dispatched
        // through the `### expr` table).
        SlotScope::Attr => match name {
            "name" => Some(SlotShape::Scalar),
            "value" => Some(SlotShape::Scalar),
            // 0-based loop ordinal (see [`RenderContext::index`]).
            "index" => Some(SlotShape::Scalar),
            _ => None,
        },
        // A single node-child element: `value` renders the child expression
        // (dispatched through the `### expr` table).
        SlotScope::Child => match name {
            "value" => Some(SlotShape::Scalar),
            // 0-based loop ordinal (see [`RenderContext::index`]).
            "index" => Some(SlotShape::Scalar),
            _ => None,
        },
        // A single array-literal element: `value` renders the element
        // expression (dispatched through the `### expr` table).
        SlotScope::ArrayElem => match name {
            "value" => Some(SlotShape::Scalar),
            // 0-based loop ordinal (see [`RenderContext::index`]).
            "index" => Some(SlotShape::Scalar),
            _ => None,
        },
        // A raw / verbatim top-level item: `value` is the verbatim code string
        // (a scalar leaf), emitted unchanged.
        SlotScope::Raw => match name {
            "value" => Some(SlotShape::Scalar),
            _ => None,
        },
        // A lambda: `params` loops the shared `param` item slot (reusing
        // [`SlotScope::Param`], so a lambda parameter renders through the same
        // `name`/`type` sub-slots as a function parameter); `ret_type` is a
        // scalar (the optional declared return type, guarded by `has_ret_type`);
        // `body` loops the recursive `statement` item slot (the general
        // statement-block form).
        SlotScope::Lambda => match name {
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
        assert_eq!(
            slot_binding("lhs", SlotScope::Expr),
            Some(SlotShape::Scalar)
        );
        assert_eq!(
            slot_binding("rhs", SlotScope::Expr),
            Some(SlotShape::Scalar)
        );
        assert_eq!(
            slot_binding("operand", SlotScope::Expr),
            Some(SlotShape::Scalar)
        );
        assert_eq!(
            slot_binding("obj", SlotScope::Expr),
            Some(SlotShape::Scalar)
        );
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
        // Exactly 13 statement kinds; guard against accidental drift.
        assert_eq!(StatementKind::all().len(), 13);
        // Spot-check the kind spellings used by the `stmt is <kind>` fact.
        assert_eq!(StatementKind::ForEach.as_str(), "foreach");
        assert_eq!(StatementKind::Switch.as_str(), "switch");
        assert_eq!(StatementKind::Expr.as_str(), "expr");
        assert_eq!(StatementKind::Raw.as_str(), "raw");
    }

    #[test]
    fn statement_scope_binds_sub_slots() {
        // Scalar sub-slots.
        for name in [
            "name",
            "binding",
            "let_type",
            "value",
            "cond",
            "iterable",
            "scrutinee",
            "else",
            "init",
            "step",
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
                meta: crate::ast::Meta::new(),
            })
            .kind(),
            ItemKind::Function
        );
        assert_eq!(
            Item::Struct {
                name: "S".into(),
                visibility: Visibility::Public,
                fields: vec![],
                attributes: Vec::new(),
                meta: crate::ast::Meta::new(),
            }
            .kind(),
            ItemKind::Struct
        );
        assert_eq!(
            Item::Enum {
                name: "E".into(),
                visibility: Visibility::Public,
                variants: vec![],
                attributes: Vec::new(),
                meta: crate::ast::Meta::new(),
            }
            .kind(),
            ItemKind::Enum
        );
        assert_eq!(
            Item::TypeDef {
                name: "T".into(),
                target: Type::Primitive(Primitive::I32),
                meta: crate::ast::Meta::new(),
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
                meta: crate::ast::Meta::new(),
            }
            .kind(),
            ItemKind::Const
        );
        assert_eq!(
            Item::Use {
                path: "std::io".into(),
                items: vec![],
                alias: None,
                meta: crate::ast::Meta::new(),
            }
            .kind(),
            ItemKind::Use
        );
    }

    #[test]
    fn item_kind_set_is_closed_and_spellings_match_headings() {
        assert_eq!(ItemKind::all().len(), 8);
        // Each `item is <kind>` spelling and its `## <Heading>` are consistent.
        for kind in ItemKind::all() {
            assert!(!kind.as_str().is_empty());
            assert_eq!(kind.heading().to_ascii_lowercase(), kind.as_str());
            assert_eq!(kind.scope(), kind.scope());
        }
        assert_eq!(ItemKind::TypeDef.heading(), "TypeDef");
        assert_eq!(ItemKind::TypeDef.as_str(), "typedef");
        assert_eq!(ItemKind::Raw.heading(), "Raw");
        assert_eq!(ItemKind::Raw.as_str(), "raw");
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

    #[test]
    fn assign_kind_and_lvalue_validation() {
        // A `Ref`/`Field`/`Index` target is a valid lvalue.
        for target in [
            Expr::Ref("x".into()),
            Expr::Field {
                obj: Box::new(Expr::Ref("o".into())),
                field: "f".into(),
            },
            Expr::Index {
                obj: Box::new(Expr::Ref("xs".into())),
                index: Box::new(Expr::IntLiteral("0".into())),
            },
        ] {
            assert!(is_lvalue(&target));
            let stmt = Statement::assign(target, Expr::IntLiteral("1".into())).expect("lvalue");
            assert_eq!(stmt.kind(), StatementKind::Assign);
        }
        // A non-lvalue target (a literal, a call, an operator expr) is rejected.
        for bad in [
            Expr::IntLiteral("1".into()),
            Expr::Call {
                callee: Box::new(Expr::Ref("f".into())),
                args: vec![],
            },
            Expr::Binary {
                op: BinaryOp::Add,
                lhs: Box::new(Expr::Ref("a".into())),
                rhs: Box::new(Expr::Ref("b".into())),
            },
        ] {
            assert!(!is_lvalue(&bad));
            assert!(matches!(
                Statement::assign(bad, Expr::IntLiteral("1".into())),
                Err(AstError::NotAnLvalue { .. })
            ));
        }
    }

    #[test]
    fn assign_binds_target_and_sub_part_slots() {
        assert_eq!(
            slot_binding("target", SlotScope::Statement),
            Some(SlotShape::Scalar)
        );
        // One-level structural sub-part slots resolve as scalars at statement
        // scope (Part 2).
        for name in ["value.lhs", "value.rhs", "value.op"] {
            assert_eq!(
                slot_binding(name, SlotScope::Statement),
                Some(SlotShape::Scalar),
                "{name} should be scalar at statement scope"
            );
        }
        // A two-level path is not bound (one level only).
        assert_eq!(slot_binding("value.lhs.rhs", SlotScope::Statement), None);
    }

    #[test]
    fn cast_kind_and_slots() {
        let c = Expr::Cast {
            value: Box::new(Expr::Ref("x".into())),
            ty: Type::Primitive(Primitive::I64),
        };
        assert_eq!(c.kind(), ExprKind::Cast);
        assert_eq!(ExprKind::Cast.as_str(), "cast");
        assert_eq!(slot_binding("ty", SlotScope::Expr), Some(SlotShape::Scalar));
    }

    #[test]
    fn struct_lit_kind_and_slots() {
        let s = Expr::StructLit {
            type_name: "Account".into(),
            fields: vec![FieldInit {
                name: "balance".into(),
                value: Expr::IntLiteral("0".into()),
                meta: crate::ast::Meta::new(),
            }],
            meta: crate::ast::Meta::new(),
        };
        assert_eq!(s.kind(), ExprKind::StructLit);
        assert_eq!(ExprKind::StructLit.as_str(), "struct_lit");
        assert_eq!(
            slot_binding("type_name", SlotScope::Expr),
            Some(SlotShape::Scalar)
        );
        match slot_binding("fields", SlotScope::Expr) {
            Some(SlotShape::Sequence {
                item_slot,
                item_scope,
            }) => {
                assert_eq!(item_slot, "field_init");
                assert_eq!(item_scope, SlotScope::FieldInit);
            }
            other => panic!("fields should be a field_init sequence, got {other:?}"),
        }
        // A field-init element exposes `name` and `value`.
        assert_eq!(
            slot_binding("name", SlotScope::FieldInit),
            Some(SlotShape::Scalar)
        );
        assert_eq!(
            slot_binding("value", SlotScope::FieldInit),
            Some(SlotShape::Scalar)
        );
        assert_eq!(slot_binding("bogus", SlotScope::FieldInit), None);
    }

    #[test]
    fn node_and_text_kinds_and_slots() {
        let node = Expr::Node {
            name: "div".into(),
            attrs: vec![Attr {
                name: "class".into(),
                value: Expr::StringLiteral("x".into()),
                meta: crate::ast::Meta::new(),
            }],
            children: vec![Expr::Text(Box::new(Expr::StringLiteral("hi".into())))],
            meta: crate::ast::Meta::new(),
        };
        assert_eq!(node.kind(), ExprKind::Node);
        assert_eq!(ExprKind::Node.as_str(), "node");
        let text = Expr::Text(Box::new(Expr::StringLiteral("hi".into())));
        assert_eq!(text.kind(), ExprKind::Text);
        assert_eq!(ExprKind::Text.as_str(), "text");

        // A node's `node_name` is a scalar leaf in expression scope; `attrs`
        // and `children` are sequences looping the `attr` / `child` item slots.
        assert_eq!(
            slot_binding("node_name", SlotScope::Expr),
            Some(SlotShape::Scalar)
        );
        match slot_binding("attrs", SlotScope::Expr) {
            Some(SlotShape::Sequence {
                item_slot,
                item_scope,
            }) => {
                assert_eq!(item_slot, "attr");
                assert_eq!(item_scope, SlotScope::Attr);
            }
            other => panic!("attrs should be an attr sequence, got {other:?}"),
        }
        match slot_binding("children", SlotScope::Expr) {
            Some(SlotShape::Sequence {
                item_slot,
                item_scope,
            }) => {
                assert_eq!(item_slot, "child");
                assert_eq!(item_scope, SlotScope::Child);
            }
            other => panic!("children should be a child sequence, got {other:?}"),
        }
        // An attr element exposes `name` and `value`; a child exposes `value`.
        assert_eq!(
            slot_binding("name", SlotScope::Attr),
            Some(SlotShape::Scalar)
        );
        assert_eq!(
            slot_binding("value", SlotScope::Attr),
            Some(SlotShape::Scalar)
        );
        assert_eq!(
            slot_binding("value", SlotScope::Child),
            Some(SlotShape::Scalar)
        );
        assert_eq!(slot_binding("bogus", SlotScope::Child), None);
    }

    #[test]
    fn node_equality_ignores_metadata() {
        // Two structurally-equal nodes are equal regardless of metadata (tree
        // nodes follow the same metadata-ignoring equality as the rest of the
        // AST).
        let a = Expr::Node {
            name: "p".into(),
            attrs: vec![],
            children: vec![],
            meta: crate::ast::Meta::new(),
        };
        let b = Expr::Node {
            name: "p".into(),
            attrs: vec![],
            children: vec![],
            meta: crate::ast::Meta::new().with("void", "true"),
        };
        assert_eq!(a, b);
    }

    #[test]
    fn tree_item_kind_and_scope() {
        let item = Item::Tree(Expr::Node {
            name: "html".into(),
            attrs: vec![],
            children: vec![],
            meta: crate::ast::Meta::new(),
        });
        assert_eq!(item.kind(), ItemKind::Tree);
        assert_eq!(ItemKind::Tree.as_str(), "tree");
        assert_eq!(ItemKind::Tree.scope(), SlotScope::Node);
    }

    // ---- Part 1: arrays ------------------------------------------------

    #[test]
    fn array_literal_kind_and_slots() {
        let a = Expr::ArrayLit {
            elems: vec![Expr::IntLiteral("1".into())],
            meta: crate::ast::Meta::new(),
        };
        assert_eq!(a.kind(), ExprKind::ArrayLit);
        assert_eq!(ExprKind::ArrayLit.as_str(), "array");
        // `elems` loops the `array_elem` item slot in ArrayElem scope.
        match slot_binding("elems", SlotScope::Expr) {
            Some(SlotShape::Sequence {
                item_slot,
                item_scope,
            }) => {
                assert_eq!(item_slot, "array_elem");
                assert_eq!(item_scope, SlotScope::ArrayElem);
            }
            other => panic!("elems should be an array_elem sequence, got {other:?}"),
        }
        assert_eq!(
            slot_binding("value", SlotScope::ArrayElem),
            Some(SlotShape::Scalar)
        );
        assert_eq!(slot_binding("bogus", SlotScope::ArrayElem), None);
    }

    #[test]
    fn array_type_slots() {
        // The array type exposes `elem` (recursive) and `len` (textual leaf).
        assert_eq!(slot_binding("elem", SlotScope::Type), Some(SlotShape::Scalar));
        assert_eq!(slot_binding("len", SlotScope::Type), Some(SlotShape::Scalar));
        let sized = Type::Array {
            elem: Box::new(Type::Primitive(Primitive::I32)),
            len: Some("3".into()),
        };
        let unsized_ty = Type::Array {
            elem: Box::new(Type::Primitive(Primitive::I32)),
            len: None,
        };
        // Structural equality distinguishes sized from unsized.
        assert_ne!(sized, unsized_ty);
        assert_eq!(sized, sized.clone());
    }

    #[test]
    fn array_literal_equality_ignores_metadata() {
        let a = Expr::ArrayLit {
            elems: vec![Expr::IntLiteral("1".into())],
            meta: crate::ast::Meta::new(),
        };
        let b = Expr::ArrayLit {
            elems: vec![Expr::IntLiteral("1".into())],
            meta: crate::ast::Meta::new().with("origin", "layer"),
        };
        assert_eq!(a, b);
    }

    // ---- Part 2: enum payloads -----------------------------------------

    #[test]
    fn variant_payload_kinds() {
        assert_eq!(VariantPayload::None.kind(), VariantKind::Unit);
        assert_eq!(
            VariantPayload::Tuple(vec![Type::Primitive(Primitive::I32)]).kind(),
            VariantKind::Tuple
        );
        assert_eq!(
            VariantPayload::Struct(vec![]).kind(),
            VariantKind::Struct
        );
        assert_eq!(VariantKind::Unit.as_str(), "unit");
        assert_eq!(VariantKind::Tuple.as_str(), "tuple");
        assert_eq!(VariantKind::Struct.as_str(), "struct");
    }

    #[test]
    fn variant_equality_uses_name_and_payload_ignores_meta() {
        let a = Variant {
            name: "Circle".into(),
            payload: VariantPayload::Tuple(vec![Type::Primitive(Primitive::I32)]),
            meta: crate::ast::Meta::new(),
        };
        let b = Variant {
            name: "Circle".into(),
            payload: VariantPayload::Tuple(vec![Type::Primitive(Primitive::I32)]),
            meta: crate::ast::Meta::new().with("k", "v"),
        };
        // Same name + payload, different metadata -> equal.
        assert_eq!(a, b);
        // Different payload shape -> not equal.
        let c = Variant {
            name: "Circle".into(),
            payload: VariantPayload::None,
            meta: crate::ast::Meta::new(),
        };
        assert_ne!(a, c);
    }

    #[test]
    fn variant_payload_slots() {
        // A variant exposes `name` plus its two payload sequences.
        assert_eq!(
            slot_binding("name", SlotScope::Variant),
            Some(SlotShape::Scalar)
        );
        match slot_binding("payload_types", SlotScope::Variant) {
            Some(SlotShape::Sequence {
                item_slot,
                item_scope,
            }) => {
                assert_eq!(item_slot, "payload_type");
                assert_eq!(item_scope, SlotScope::PayloadType);
            }
            other => panic!("payload_types should be a payload_type sequence, got {other:?}"),
        }
        match slot_binding("payload_fields", SlotScope::Variant) {
            Some(SlotShape::Sequence {
                item_slot,
                item_scope,
            }) => {
                assert_eq!(item_slot, "payload_field");
                // Payload fields reuse the struct-field scope.
                assert_eq!(item_scope, SlotScope::Field);
            }
            other => panic!("payload_fields should be a payload_field sequence, got {other:?}"),
        }
        assert_eq!(
            slot_binding("type", SlotScope::PayloadType),
            Some(SlotShape::Scalar)
        );
        assert_eq!(slot_binding("bogus", SlotScope::PayloadType), None);
    }

    // ---- Part 3: structured use ----------------------------------------

    #[test]
    fn use_slots_and_equality() {
        // The Use scope exposes `path`/`alias` (scalar) and `items` (sequence).
        assert_eq!(slot_binding("path", SlotScope::Use), Some(SlotShape::Scalar));
        assert_eq!(slot_binding("alias", SlotScope::Use), Some(SlotShape::Scalar));
        match slot_binding("items", SlotScope::Use) {
            Some(SlotShape::Sequence {
                item_slot,
                item_scope,
            }) => {
                assert_eq!(item_slot, "use_item");
                assert_eq!(item_scope, SlotScope::UseItem);
            }
            other => panic!("items should be a use_item sequence, got {other:?}"),
        }
        // A use-item element exposes `name` and `alias`.
        assert_eq!(
            slot_binding("name", SlotScope::UseItem),
            Some(SlotShape::Scalar)
        );
        assert_eq!(
            slot_binding("alias", SlotScope::UseItem),
            Some(SlotShape::Scalar)
        );
        assert_eq!(slot_binding("bogus", SlotScope::UseItem), None);
    }

    #[test]
    fn use_equality_ignores_metadata_compares_structure() {
        let a = Item::Use {
            path: "std::io".into(),
            items: vec![UseItem {
                name: "Read".into(),
                alias: Some("R".into()),
                meta: crate::ast::Meta::new(),
            }],
            alias: None,
            meta: crate::ast::Meta::new(),
        };
        let b = Item::Use {
            path: "std::io".into(),
            items: vec![UseItem {
                name: "Read".into(),
                alias: Some("R".into()),
                meta: crate::ast::Meta::new().with("k", "v"),
            }],
            alias: None,
            meta: crate::ast::Meta::new().with("x", "y"),
        };
        assert_eq!(a, b);
        // A different alias makes the imports unequal.
        let c = Item::Use {
            path: "std::io".into(),
            items: vec![],
            alias: Some("io".into()),
            meta: crate::ast::Meta::new(),
        };
        assert_ne!(a, c);
    }

    // ---- Type attributes -----------------------------------------------

    #[test]
    fn type_attribute_roundtrips_via_name() {
        for attr in TypeAttribute::all() {
            assert_eq!(TypeAttribute::from_name(attr.as_str()), Some(attr));
            assert!(!attr.as_str().is_empty());
        }
        assert_eq!(TypeAttribute::from_name("nope"), None);
    }

    #[test]
    fn type_class_roundtrips_via_name() {
        for class in TypeClass::all() {
            assert_eq!(TypeClass::from_name(class.as_str()), Some(class));
            assert!(!class.as_str().is_empty());
        }
        assert_eq!(TypeClass::from_name("nope"), None);
    }

    #[test]
    fn primitive_class_maps_every_frozen_primitive() {
        // The classification is TOTAL over the frozen 26-primitive set: every
        // primitive maps to exactly the expected class. This locks the mapping
        // (the exhaustive `match` in `Primitive::class` already makes a missing
        // primitive a compile error; this asserts the concrete groupings).
        use Primitive::*;
        let expect: &[(Primitive, TypeClass)] = &[
            (I8, TypeClass::SignedInt),
            (I16, TypeClass::SignedInt),
            (I32, TypeClass::SignedInt),
            (I64, TypeClass::SignedInt),
            (I128, TypeClass::SignedInt),
            (Isize, TypeClass::SignedInt),
            (U8, TypeClass::UnsignedInt),
            (U16, TypeClass::UnsignedInt),
            (U32, TypeClass::UnsignedInt),
            (U64, TypeClass::UnsignedInt),
            (U128, TypeClass::UnsignedInt),
            (Usize, TypeClass::UnsignedInt),
            (Byte, TypeClass::UnsignedInt),
            (F16, TypeClass::Float),
            (Bf16, TypeClass::Float),
            (F32, TypeClass::Float),
            (F64, TypeClass::Float),
            (F128, TypeClass::Float),
            (Bool, TypeClass::Bool),
            (Char, TypeClass::Char),
            (Str, TypeClass::String),
            (Bytes, TypeClass::Bytes),
            (Void, TypeClass::Unit),
            (Never, TypeClass::Never),
            (Ptr, TypeClass::Ptr),
            (Fnptr, TypeClass::FnPtr),
        ];
        // Every frozen primitive is covered exactly once.
        assert_eq!(expect.len(), Primitive::all().len());
        for (p, want) in expect {
            assert_eq!(p.class(), *want, "{p:?} should classify as {want:?}");
            assert_eq!(
                Type::Primitive(*p).class(),
                *want,
                "Type::Primitive({p:?}) should classify as {want:?}"
            );
        }
    }

    #[test]
    fn type_class_maps_every_compound_variant() {
        assert_eq!(Type::Named("Foo".to_string()).class(), TypeClass::Named);
        assert_eq!(
            Type::Pointer(Box::new(Type::Primitive(Primitive::I32))).class(),
            TypeClass::Ptr
        );
        assert_eq!(
            Type::FnPtr {
                params: vec![],
                ret: Box::new(Type::Primitive(Primitive::Void)),
            }
            .class(),
            TypeClass::FnPtr
        );
        assert_eq!(
            Type::Array {
                elem: Box::new(Type::Primitive(Primitive::I32)),
                len: Some("3".to_string()),
            }
            .class(),
            TypeClass::Array
        );
    }

    #[test]
    fn type_attribute_set_is_the_closed_kernel_superset() {
        // Exactly 8 attributes, in canonical order with canonical spellings.
        let all = TypeAttribute::all();
        assert_eq!(all.len(), 8);
        assert_eq!(
            all.map(|a| a.as_str()),
            [
                "displayable",
                "equatable",
                "comparable",
                "hashable",
                "cloneable",
                "copyable",
                "hasdefault",
                "iterable"
            ]
        );
    }

    #[test]
    fn struct_and_enum_scopes_bind_attributes() {
        // `attributes` loops the `attribute` item slot in Attribute scope, at
        // both struct and enum scope.
        for scope in [SlotScope::Struct, SlotScope::Enum] {
            match slot_binding("attributes", scope) {
                Some(SlotShape::Sequence {
                    item_slot,
                    item_scope,
                }) => {
                    assert_eq!(item_slot, "attribute");
                    assert_eq!(item_scope, SlotScope::Attribute);
                }
                other => panic!("attributes should be an attribute sequence, got {other:?}"),
            }
        }
        // The attribute element exposes only `name`.
        assert_eq!(
            slot_binding("name", SlotScope::Attribute),
            Some(SlotShape::Scalar)
        );
        assert_eq!(slot_binding("bogus", SlotScope::Attribute), None);
    }

    #[test]
    fn struct_and_enum_equality_ignores_attributes() {
        // Two structs identical but for their attributes are structurally
        // equal (attributes are ignored exactly as metadata is).
        let plain = Item::Struct {
            name: "P".into(),
            visibility: Visibility::Public,
            fields: vec![],
            attributes: Vec::new(),
            meta: crate::ast::Meta::new(),
        };
        let derived = Item::Struct {
            name: "P".into(),
            visibility: Visibility::Public,
            fields: vec![],
            attributes: vec![TypeAttribute::Displayable, TypeAttribute::Cloneable],
            meta: crate::ast::Meta::new(),
        };
        assert_eq!(plain, derived);

        let enum_plain = Item::Enum {
            name: "E".into(),
            visibility: Visibility::Private,
            variants: vec![],
            attributes: Vec::new(),
            meta: crate::ast::Meta::new(),
        };
        let enum_derived = Item::Enum {
            name: "E".into(),
            visibility: Visibility::Private,
            variants: vec![],
            attributes: vec![TypeAttribute::Displayable],
            meta: crate::ast::Meta::new(),
        };
        assert_eq!(enum_plain, enum_derived);
    }
}
