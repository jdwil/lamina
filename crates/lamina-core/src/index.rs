//! The per-unit **symbol + reference index** and the closed render helpers
//! (Part 2 of the metadata/helpers spec).
//!
//! The recursive renderer is *local*: each node renders from its own subtree.
//! A few reconstructions (inlining an anonymous function, counting how often a
//! function is used as a value, resolving a field's declared type) need to
//! cross that local boundary and consult the whole unit. This module builds a
//! read-only index of the unit **once** and exposes a **closed** set of
//! engine-provided helpers over it.
//!
//! The helpers are *pure* — they never mutate the AST — and *fixed*: a language
//! definition may only *call* the named helpers the engine provides, never
//! define new ones. This keeps the escape hatch controlled and the `.mdl`
//! language non-Turing-complete: helpers are named, fixed-arity functions, not
//! a general query language.
//!
//! # The closed helper set
//!
//! - Template side (produce rendered output): `resolve_fnptr(<expr>)`,
//!   `escape(<expr>, <style>)`.
//! - Predicate side (produce facts): `fnptr_ref_count(<expr>) is <n>`,
//!   `type_of(<expr>) is <type>`.
//! - Symbol/type resolution (both sides): `resolve(<name>) is <kind>`,
//!   `field_type(<struct>, <field>)`.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

use crate::ast::{Expr, File, Item, Statement, Type};

/// The mutable, per-`emit` render state threaded (by shared reference, with
/// interior mutability) through every resolver: the current pass, the
/// author-named output-region buffers, and the memoized fresh-name allocations.
///
/// This is the ONLY mutable state the otherwise-pure renderer carries, and it
/// exists solely to serve the two DUMB multi-pass primitives: (1) a
/// `current_pass` that scopes which rules are active, and (2) a `BTreeMap` of
/// author-named region buffers assembled into the final output. The engine
/// attaches no meaning to any pass or region name.
#[derive(Debug, Clone, Default)]
pub struct RenderState {
    /// The pass currently being rendered, or `None` in legacy single-pass mode
    /// (a definition with no `## Passes` section). Rules annotated `pass: X`
    /// are active only when this equals `X`.
    current_pass: Option<String>,
    /// Author-named output-region buffers, created on first emission to them.
    /// A `BTreeMap` keeps iteration deterministic; the definition's declared
    /// `layout` drives the actual assembly order.
    regions: BTreeMap<String, String>,
    /// Memoized fresh-name allocations keyed by `(prefix, key)`. The SAME
    /// `(prefix, key)` returns the SAME identifier no matter which pass or
    /// region requests it — this is what lets a hoisted definition (emitted in
    /// one region during one pass) and its inline reference (emitted in another
    /// region/pass) coordinate on a single generated name.
    fresh: BTreeMap<(String, String), String>,
    /// A monotonically-increasing counter making each distinct `(prefix, key)`
    /// allocation unique within the unit.
    fresh_counter: usize,
}

/// A read-only index of one compilation unit ([`File`]): a map of top-level
/// item names to their definitions, plus a count of how many times each
/// function name is referenced *as a value* (a function-pointer reference).
///
/// Built once per `emit` and threaded (by shared reference) into every
/// resolver, so the closed helpers can answer cross-item questions without the
/// renderer losing its otherwise-local character.
///
/// It also owns the per-`emit` [`RenderState`] behind a [`RefCell`], giving the
/// otherwise-immutable `&UnitIndex` that every resolver holds the interior
/// mutability the multi-pass driver and `fresh_name` helper need — without
/// threading a second mutable reference through every render function.
#[derive(Debug, Default)]
pub struct UnitIndex<'a> {
    /// Top-level items by name (functions, structs, enums, typedefs, consts).
    /// A `use` import has no binding name and is omitted.
    items: HashMap<&'a str, &'a Item>,
    /// How many times each name appears as a *function-pointer value* — a bare
    /// [`Expr::Ref`] in a value position (a call/struct-lit argument, an
    /// initializer, a returned value, …) rather than the callee of a direct
    /// call. Used by the `fnptr_ref_count(<expr>) is <n>` inlining-policy fact.
    fnptr_ref_counts: HashMap<String, usize>,
    /// The mutable render state (current pass + region buffers + fresh-name
    /// memo). Interior-mutable so a shared `&UnitIndex` can drive passes and
    /// allocate names.
    state: RefCell<RenderState>,
}

impl<'a> UnitIndex<'a> {
    /// Builds the index for `file`: records every named top-level item and
    /// counts function-pointer references across every function body.
    pub fn build(file: &'a File) -> Self {
        let mut items: HashMap<&'a str, &'a Item> = HashMap::new();
        for item in &file.items {
            if let Some(name) = item_name(item) {
                items.insert(name, item);
            }
        }
        let mut fnptr_ref_counts: HashMap<String, usize> = HashMap::new();
        for item in &file.items {
            if let Item::Function(f) = item {
                for stmt in &f.body {
                    count_fnptr_refs_in_stmt(stmt, &items, &mut fnptr_ref_counts);
                }
            }
            if let Item::Const { value, .. } = item {
                count_fnptr_refs_in_expr(value, &items, &mut fnptr_ref_counts);
            }
            if let Item::Tree(expr) = item {
                count_fnptr_refs_in_expr(expr, &items, &mut fnptr_ref_counts);
            }
        }
        UnitIndex {
            items,
            fnptr_ref_counts,
            state: RefCell::new(RenderState::default()),
        }
    }

    /// Resolves a top-level item by name, if present in this unit.
    pub fn item(&self, name: &str) -> Option<&'a Item> {
        self.items.get(name).copied()
    }

    /// Resolves a name to a top-level [`crate::ast::Function`] definition, if it
    /// names a function in this unit. Used by `resolve_fnptr` to inline a
    /// function referenced as a value.
    pub fn function(&self, name: &str) -> Option<&'a crate::ast::Function> {
        match self.items.get(name) {
            Some(Item::Function(f)) => Some(f),
            _ => None,
        }
    }

    /// How many times `name` is referenced as a function-pointer value in this
    /// unit (answers `fnptr_ref_count(<expr>) is <n>`). Zero if never.
    pub fn fnptr_ref_count(&self, name: &str) -> usize {
        self.fnptr_ref_counts.get(name).copied().unwrap_or(0)
    }

    /// The declared type of `field` on the struct named `struct_name`, if both
    /// the struct and the field exist (answers `field_type(<struct>, <field>)`).
    pub fn field_type(&self, struct_name: &str, field: &str) -> Option<&'a Type> {
        match self.items.get(struct_name) {
            Some(Item::Struct { fields, .. }) => {
                fields.iter().find(|f| f.name == field).map(|f| &f.ty)
            }
            _ => None,
        }
    }

    /// The resolution *kind* of `name` — the `## <Item>` kind spelling
    /// (`function`, `struct`, `enum`, `typedef`, `const`) — if `name` resolves
    /// to a top-level item (answers `resolve(<name>) is <kind>`).
    pub fn resolve_kind(&self, name: &str) -> Option<&'static str> {
        self.items.get(name).map(|item| item.kind().as_str())
    }

    /// Sets the pass currently being rendered (the multi-pass driver calls this
    /// once per pass before re-rendering the unit). `None` restores legacy
    /// single-pass mode.
    pub fn set_current_pass(&self, pass: Option<String>) {
        self.state.borrow_mut().current_pass = pass;
    }

    /// The pass currently being rendered, or `None` in legacy single-pass mode.
    /// Read by the emitter to pass into [`crate::lang::WhenTable::select`] so a
    /// row's `pass:` annotation gates whether it is considered.
    pub fn current_pass(&self) -> Option<String> {
        self.state.borrow().current_pass.clone()
    }

    /// Appends `text` to the author-named output region `region`, creating the
    /// buffer on first use. This is how a rule annotated `region: X` routes its
    /// rendered output away from the inline position and into a named buffer the
    /// definition later assembles via its declared `layout`.
    pub fn emit_to_region(&self, region: &str, text: &str) {
        let mut state = self.state.borrow_mut();
        state
            .regions
            .entry(region.to_string())
            .or_default()
            .push_str(text);
    }

    /// Returns a unit-stable, unique identifier for `(prefix, key)`, MEMOIZED:
    /// the same `(prefix, key)` always returns the SAME identifier for the whole
    /// `emit`, regardless of which pass or region requests it. This is the one
    /// cross-pass/region coordination the mechanism provides — it lets a hoisted
    /// helper definition (emitted into one region during one pass) and its
    /// inline reference (emitted elsewhere) agree on a single generated name.
    ///
    /// The identifier is `<prefix>_<n>` where `n` is a monotonically-increasing
    /// per-unit counter, so distinct `(prefix, key)` pairs never collide.
    pub fn fresh_name(&self, prefix: &str, key: &str) -> String {
        let mut state = self.state.borrow_mut();
        let map_key = (prefix.to_string(), key.to_string());
        if let Some(existing) = state.fresh.get(&map_key) {
            return existing.clone();
        }
        let n = state.fresh_counter;
        state.fresh_counter += 1;
        let name = format!("{prefix}_{n}");
        state.fresh.insert(map_key, name.clone());
        name
    }

    /// Consumes and returns the accumulated region buffers (draining the map),
    /// for final assembly by the multi-pass driver.
    pub fn take_regions(&self) -> BTreeMap<String, String> {
        std::mem::take(&mut self.state.borrow_mut().regions)
    }
}

/// The binding name of a top-level item, or `None` for a `use` import (which
/// binds no name the index can key on).
fn item_name(item: &Item) -> Option<&str> {
    match item {
        Item::Function(f) => Some(&f.name),
        Item::Struct { name, .. }
        | Item::Enum { name, .. }
        | Item::TypeDef { name, .. }
        | Item::Const { name, .. } => Some(name),
        Item::Use { .. } => None,
        // A top-level tree value binds no name the index can key on.
        Item::Tree(_) => None,
        // A raw / verbatim item is an opaque code string — it binds no name the
        // index can key on.
        Item::Raw { .. } => None,
    }
}

/// Recursively counts function-pointer references within a statement.
fn count_fnptr_refs_in_stmt(
    stmt: &Statement,
    items: &HashMap<&str, &Item>,
    counts: &mut HashMap<String, usize>,
) {
    match stmt {
        Statement::Block(body) => {
            for s in body {
                count_fnptr_refs_in_stmt(s, items, counts);
            }
        }
        Statement::Let { value: Some(v), .. } => count_fnptr_refs_in_expr(v, items, counts),
        Statement::Let { value: None, .. } => {}
        Statement::Return(Some(v)) => count_fnptr_refs_in_expr(v, items, counts),
        Statement::Return(None) => {}
        Statement::If {
            cond,
            then_block,
            else_block,
        } => {
            count_fnptr_refs_in_expr(cond, items, counts);
            for s in then_block {
                count_fnptr_refs_in_stmt(s, items, counts);
            }
            if let Some(e) = else_block {
                count_fnptr_refs_in_stmt(e, items, counts);
            }
        }
        Statement::While { cond, body } => {
            count_fnptr_refs_in_expr(cond, items, counts);
            for s in body {
                count_fnptr_refs_in_stmt(s, items, counts);
            }
        }
        Statement::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init {
                count_fnptr_refs_in_stmt(i, items, counts);
            }
            if let Some(c) = cond {
                count_fnptr_refs_in_expr(c, items, counts);
            }
            if let Some(s) = step {
                count_fnptr_refs_in_stmt(s, items, counts);
            }
            for s in body {
                count_fnptr_refs_in_stmt(s, items, counts);
            }
        }
        Statement::ForEach { iterable, body, .. } => {
            count_fnptr_refs_in_expr(iterable, items, counts);
            for s in body {
                count_fnptr_refs_in_stmt(s, items, counts);
            }
        }
        Statement::Switch {
            scrutinee,
            cases,
            default,
        } => {
            count_fnptr_refs_in_expr(scrutinee, items, counts);
            for case in cases {
                count_fnptr_refs_in_expr(&case.value, items, counts);
                for s in &case.body {
                    count_fnptr_refs_in_stmt(s, items, counts);
                }
            }
            if let Some(d) = default {
                for s in d {
                    count_fnptr_refs_in_stmt(s, items, counts);
                }
            }
        }
        Statement::Assign { target, value } => {
            // The target is an lvalue place, not a value position, so it is not
            // a fnptr value; only the assigned value counts.
            let _ = target;
            count_fnptr_refs_in_expr(value, items, counts);
        }
        Statement::Expr(e) => count_fnptr_refs_in_expr(e, items, counts),
        Statement::Break | Statement::Continue => {}
        // A raw statement is an opaque verbatim code string — nothing to walk.
        Statement::Raw { .. } => {}
    }
}

/// Recursively counts function-pointer references within an expression.
///
/// A bare [`Expr::Ref`] naming a top-level *function* counts as a
/// function-pointer value **except** when it is the direct callee of a
/// [`Expr::Call`] (a direct call is not a value reference — C's "function decay"
/// only makes the *value* use a pointer). So a call's callee is skipped while
/// its arguments are still scanned.
fn count_fnptr_refs_in_expr(
    expr: &Expr,
    items: &HashMap<&str, &Item>,
    counts: &mut HashMap<String, usize>,
) {
    match expr {
        Expr::Ref(name) => {
            if matches!(items.get(name.as_str()), Some(Item::Function(_))) {
                *counts.entry(name.clone()).or_insert(0) += 1;
            }
        }
        Expr::Field { obj, .. } => count_fnptr_refs_in_expr(obj, items, counts),
        Expr::Index { obj, index } => {
            count_fnptr_refs_in_expr(obj, items, counts);
            count_fnptr_refs_in_expr(index, items, counts);
        }
        Expr::Call { callee, args } => {
            // A direct call's callee is NOT a value reference (no decay); only
            // its arguments are scanned for fnptr values.
            if !matches!(callee.as_ref(), Expr::Ref(_)) {
                count_fnptr_refs_in_expr(callee, items, counts);
            }
            for a in args {
                count_fnptr_refs_in_expr(a, items, counts);
            }
        }
        Expr::Unary { operand, .. } => count_fnptr_refs_in_expr(operand, items, counts),
        Expr::Binary { lhs, rhs, .. } => {
            count_fnptr_refs_in_expr(lhs, items, counts);
            count_fnptr_refs_in_expr(rhs, items, counts);
        }
        Expr::Cast { value, .. } => count_fnptr_refs_in_expr(value, items, counts),
        Expr::StructLit { fields, .. } => {
            for f in fields {
                count_fnptr_refs_in_expr(&f.value, items, counts);
            }
        }
        // Tree-core nodes: an attribute value or a child may itself be a
        // function-pointer value (interpolation), so recurse into both.
        Expr::Node {
            attrs, children, ..
        } => {
            for a in attrs {
                count_fnptr_refs_in_expr(&a.value, items, counts);
            }
            for c in children {
                count_fnptr_refs_in_expr(c, items, counts);
            }
        }
        Expr::Text(inner) => count_fnptr_refs_in_expr(inner, items, counts),
        // An array literal's elements are value positions, so a bare function
        // reference among them is a fnptr value — recurse into each.
        Expr::ArrayLit { elems, .. } => {
            for e in elems {
                count_fnptr_refs_in_expr(e, items, counts);
            }
        }
        // A lambda's body is a statement block: a bare function reference in a
        // value position within the body is a fnptr value, so recurse through
        // the body statements. (Parameters are binding positions, not value
        // references, so they carry no fnptr values themselves.)
        Expr::Lambda { body, .. } => {
            for s in body {
                count_fnptr_refs_in_stmt(s, items, counts);
            }
        }
        Expr::IntLiteral(_)
        | Expr::FloatLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::StringLiteral(_)
        | Expr::CharLiteral(_)
        | Expr::NullLiteral => {}
        // A raw expression is an opaque verbatim code string — nothing to walk.
        Expr::Raw { .. } => {}
    }
}

/// The escaping style a target requests when spelling a string/char literal via
/// the `escape(<expr>, <style>)` helper.
///
/// This is the CLOSED set of styles the engine knows how to apply. It is
/// deliberately small: a target names the mode it wants and the engine performs
/// the byte-for-byte escaping. Adding a target-specific quirk is a new named
/// style here, never an open scripting facility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscapeStyle {
    /// C-family escaping (`\n`, `\t`, `\\`, `\"`, `\'`, `\r`): shared by
    /// C, Rust, TypeScript, Java, and most curly-brace targets.
    C,
    /// JSON string escaping (`\n`, `\t`, `\\`, `\"`, `\r`, and no `\'`).
    Json,
    /// No escaping — emit the contents verbatim (`raw`).
    Raw,
}

impl EscapeStyle {
    /// Resolves an escape style from its `.mdl` spelling (the second argument of
    /// `escape(<expr>, <style>)`), or `None` if it is not a known style.
    pub fn from_name(name: &str) -> Option<EscapeStyle> {
        match name {
            "c" => Some(EscapeStyle::C),
            "json" => Some(EscapeStyle::Json),
            "raw" => Some(EscapeStyle::Raw),
            _ => None,
        }
    }

    /// Escapes `s` for this style, returning the escaped contents (without any
    /// surrounding quotes — the template supplies the quote characters).
    pub fn apply(self, s: &str) -> String {
        match self {
            EscapeStyle::Raw => s.to_string(),
            EscapeStyle::C => escape_common(s, true),
            EscapeStyle::Json => escape_common(s, false),
        }
    }
}

/// Shared escaping for the C and JSON styles. When `single_quote` is `true`
/// (C style), a single quote is escaped too (so the same escaped form is valid
/// inside either `'…'` or `"…"`).
fn escape_common(s: &str, single_quote: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\'' if single_quote => out.push_str("\\'"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Function, Meta, Primitive, Visibility};

    fn func(name: &str, body: Vec<Statement>) -> Item {
        Item::Function(Function {
            name: name.to_string(),
            visibility: Visibility::Private,
            modifiers: vec![],
            params: vec![],
            return_type: Type::Primitive(Primitive::Void),
            body,
            meta: Meta::new(),
        })
    }

    #[test]
    fn indexes_named_items_and_skips_use() {
        let file = File {
            items: vec![
                func("f", vec![]),
                Item::Use {
                    path: "std".into(),
                    items: vec![],
                    alias: None,
                    meta: Meta::new(),
                },
            ],
        };
        let idx = UnitIndex::build(&file);
        assert!(idx.function("f").is_some());
        assert_eq!(idx.resolve_kind("f"), Some("function"));
        assert_eq!(idx.resolve_kind("missing"), None);
    }

    #[test]
    fn counts_fnptr_refs_but_not_direct_calls() {
        // `g` is called directly (no decay) and also passed as a value once.
        let body = vec![
            Statement::Expr(Expr::Call {
                callee: Box::new(Expr::Ref("g".into())),
                args: vec![],
            }),
            Statement::Expr(Expr::Call {
                callee: Box::new(Expr::Ref("h".into())),
                args: vec![Expr::Ref("g".into())],
            }),
        ];
        let file = File {
            items: vec![func("g", vec![]), func("h", vec![]), func("caller", body)],
        };
        let idx = UnitIndex::build(&file);
        // `g` used as a value exactly once (the argument); the direct call and
        // the `h` callee are not value references.
        assert_eq!(idx.fnptr_ref_count("g"), 1);
        assert_eq!(idx.fnptr_ref_count("h"), 0);
    }

    #[test]
    fn field_type_resolves_declared_type() {
        let file = File {
            items: vec![Item::Struct {
                name: "S".into(),
                visibility: Visibility::Public,
                attributes: Vec::new(),
                fields: vec![crate::ast::Field {
                    name: "x".into(),
                    ty: Type::Primitive(Primitive::I32),
                    visibility: Visibility::Public,
                    meta: Meta::new(),
                }],
                meta: Meta::new(),
            }],
        };
        let idx = UnitIndex::build(&file);
        assert_eq!(
            idx.field_type("S", "x"),
            Some(&Type::Primitive(Primitive::I32))
        );
        assert_eq!(idx.field_type("S", "y"), None);
        assert_eq!(idx.field_type("Missing", "x"), None);
    }

    #[test]
    fn escape_styles_apply() {
        assert_eq!(EscapeStyle::C.apply("a\nb\"c'"), "a\\nb\\\"c\\'");
        assert_eq!(EscapeStyle::Json.apply("a\nb\"c'"), "a\\nb\\\"c'");
        assert_eq!(EscapeStyle::Raw.apply("a\nb"), "a\nb");
        assert_eq!(EscapeStyle::from_name("c"), Some(EscapeStyle::C));
        assert_eq!(EscapeStyle::from_name("nope"), None);
    }

    #[test]
    fn fresh_name_is_memoized_by_prefix_and_key() {
        let file = File { items: vec![] };
        let idx = UnitIndex::build(&file);
        // Same (prefix, key) -> identical name, no matter how many times or in
        // what interleaving it is requested (the cross-pass/region contract).
        let a = idx.fresh_name("loop", "w");
        let b = idx.fresh_name("loop", "w");
        assert_eq!(a, b);
        // A different key under the same prefix is a distinct name.
        let c = idx.fresh_name("loop", "x");
        assert_ne!(a, c);
        // A different prefix is distinct too.
        let d = idx.fresh_name("go", "w");
        assert_ne!(a, d);
        // Re-requesting the first pair STILL returns the original id (memoized),
        // even after other allocations bumped the counter.
        assert_eq!(idx.fresh_name("loop", "w"), a);
    }

    #[test]
    fn regions_accumulate_and_drain_in_key_order() {
        let file = File { items: vec![] };
        let idx = UnitIndex::build(&file);
        idx.emit_to_region("body", "x");
        idx.emit_to_region("helpers", "h");
        idx.emit_to_region("body", "y");
        let regions = idx.take_regions();
        assert_eq!(regions.get("body").map(String::as_str), Some("xy"));
        assert_eq!(regions.get("helpers").map(String::as_str), Some("h"));
        // Draining empties the state.
        assert!(idx.take_regions().is_empty());
    }

    #[test]
    fn current_pass_round_trips() {
        let file = File { items: vec![] };
        let idx = UnitIndex::build(&file);
        assert_eq!(idx.current_pass(), None);
        idx.set_current_pass(Some("emit".to_string()));
        assert_eq!(idx.current_pass().as_deref(), Some("emit"));
        idx.set_current_pass(None);
        assert_eq!(idx.current_pass(), None);
    }
}
