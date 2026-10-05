# Lamina Concrete Source Syntax (Design Spec)

**Status:** design-approved (interview with JD, 2026-10). This is the agreed
concrete syntax for `.mdl` **source** files. The parser is to be built against
this spec; it produces the existing kernel AST (see `crates/lamina-core/src/ast.rs`).
Until the parser exists, programs are hand-built ASTs — this spec defines what
the *written* form will be.

## Guiding principles

- **Starting point: conventional C/Rust-family, legible** — pleasant for humans
  and agents to read/write, iterate from familiar.
- **TypeScript-shaped types** — `name: Type`, return as `): ret`.
- **One grammatical shape wherever possible:** `keyword name(parens) { braces }`.
  Every body is a brace-delimited block (no exceptions — including `case` arms).
  This keeps the parser small and the source↔AST mapping direct.
- **The kernel is minimal and paradigm-neutral.** Source expresses the KERNEL
  only: `struct`/`fn`/primitives/fnptr. Higher-level constructs (`class`, OOP,
  nice markup authoring, SDK bindings) are **layers**, not kernel syntax —
  because minimal struct+fnptr primitives are the universal donor that can be
  reconstructed UP into any paradigm (OOP, functional) per target, whereas a
  class-based kernel could not lower cleanly to C/Haskell.
- **The reviewable artifact is the raise (a projection), not raw source** — so
  the source need not be beautiful, just complete, unambiguous, and regular.

## Lexical / primitives

- Primitive type names are the **kernel's frozen set, verbatim**: `i8 i16 i32
  i64 i128 u8 u16 u32 u64 u128 isize usize f16 bf16 f32 f64 f128 bool void never
  byte bytes char str ptr fnptr`. (No second vocabulary; source uses the kernel
  spellings.)
- `//` line comments (prose; ignored).
- String literals double-quoted.

## Items

### Functions
```
public async fn add(x: i32, y: i32): i32 {
    return x + y;
}
```
- Short `fn` keyword (not `function`). Params `name: Type`, comma-separated.
- Return type after params with a **colon**: `): i32`. (TypeScript shape.)
- **Visibility** keywords `public` / `protected` / `private` precede modifiers.
- **Modifier** keywords (`async`, `const`, `unsafe`, `throws`, `extern`,
  `inline`, `generator`) precede `fn`, in source order.

### Structs
```
@equatable
@displayable
struct Point {
    x: i32,
    y: i32,
}
```
- `struct` (NOT `interface`/`class` — those are OOP, a future layer).
- Fields `name: Type`, comma-separated.

### Enums (with payloads — Rust-style)
```
enum Shape {
    Empty,                    // unit variant
    Circle(f64),              // tuple variant (positional payload)
    Rect { w: f64, h: f64 },  // struct variant (named payload)
}
```

### Const / typedef / use
```
const MAX: i32 = 100;
typedef Celsius = f64;

use http;                     // import a .mdl (module / layer / profile) by name
use ./models::{ User, Role }; // selective from a local source file
use math as m;                // aliased
```
- A single `use` imports any `.mdl` artifact (source module, layer, or profile);
  the engine resolves what it points at. Kernel source does **not** describe
  external SDKs (no stubs) — rich SDK binding is a **layer** concern.

## Type attributes & metadata (the `@` sigil)

- **Type attributes** (closed set): `@equatable`, `@displayable`, `@comparable`,
  `@hashable`, `@cloneable`, `@copyable`, `@hasdefault`, `@iterable` — one per
  line above the item.
- **Metadata** (open key→string map): `@meta(key = "value", ...)`. Same sigil,
  distinguished by the `meta(...)` form. Mostly written by layers during
  lowering, not hand-authored — kept simple, "must be expressible," not
  ergonomics-optimized.

## Expressions & operators

- Canonical infix symbols, mapping 1:1 to the kernel `BinaryOp`/`UnaryOp`.
- **Conventional operator precedence**, with **parentheses** supported for
  explicit grouping/override.
- Exotic kernel operators spelled literally: `**` (pow), `//` (floordiv),
  `>>>` (ushr).
- **Struct literal:** Rust-style named fields — `Point { x: 1, y: 2 }`.
- **Bindings:** `let x = expr;` or typed `let x: i32 = expr;`.

## Statements / control flow

Every body is a brace block. Conditions are **parenthesized**.
```
if (cond) { ... } else { ... }

while (cond) { ... }

for (let i = 0; i < 10; i = i + 1) { ... }   // C-style counted loop

foreach item in items { ... }                 // iterator loop (distinct keyword)

switch (value) {
    case 1 { ... }
    case 2 { ... }
    default { ... }
}
```
- `for` (counted) and `foreach` (iterator) are **distinct keywords** mapping to
  the two distinct kernel loop nodes.
- `switch` arms are `case <value> { block }` — braced blocks, **no fall-through**
  (each arm is a self-contained block, matching the kernel's structured switch),
  with an optional `default { block }`.

## The tree core (node / attr / text)

Regular, explicit, IR-honest (nice markup authoring is a *layer*, not kernel):
```
node div(class = "box", id = "main") {
    node p {
        text "Hello"
    }
}
```
- `node name(attr = "value", ...) { children }` — mirrors `fn name(params) { body }`.
- Attributes use `=` (a value assignment, like `@meta` and `let`).
- `text "..."` — explicit text leaf (no node-vs-text inference).

## Raw / native-interop (the escape hatch)

Verbatim target code at expression, statement, or item level. The escape hatch
is **honest-but-secondary**: it works on a single target and its portability cost
is made **visible** (via the target tag), never silent. It is a `switch` over the
build target.

**Single-arm shorthand** (the common "I just need this one target" case):
```
raw python { import numpy as np }                 // item/statement block
let arr = raw python "numpy.array([1, 2, 3])";    // expression, string form
raw rust >= 1.70 "foo.cast::<u32>()";             // with a version constraint
```

**Grouped multi-target form** (a switch over the target; cohesive alternatives):
```
raw {
    python >= 3.10 { result = (x := compute()) }
    python < 3.10  { result = compute() }
    rust           { let result = compute(); }
    else           { result = compute() }          // optional fallback
}
```
- Each arm head is `<target> [version-constraint]`; the body is verbatim.
- The engine emits the arm matching the current build target+version.
- `else` is optional. **No matching arm and no `else` is a hard error** — raw
  code is never silently dropped. (Same safety stance + grammar shape as `switch`.)

## Version-constraint grammar (shared everywhere)

One grammar, reused across `raw` target tags, the `.mdl` `lang-meta`
`target-version` band, and (future) layer dependency ranges. Adopts the
**npm/Cargo-style comparators** (broadly recognized; agents know them):
`^` (compatible-with-major), `~` (compatible-with-minor), `>=`, `<=`, `>`, `<`,
`=`, and `X - Y` hyphen ranges.

- The **comparator grammar is fixed/standard**; the **version token is opaque** —
  the engine compares against the target's own version band WITHOUT assuming the
  token is strict 3-part semver (so Rust editions `2021`, Python `3.12`, etc. all
  work). This keeps the familiar standard grammar without forcing every target
  into semver.

## The `.mdl` file family

All Lamina artifacts share the `.mdl` extension and one document structure; the
role is the penultimate extension segment:
- `<name>.mdl` — source (default, untagged)
- `<name>.lang.mdl` — language definition
- `<name>.layer.mdl` — layer
- `<name>.profile.mdl` — profile (target project structure)

## Worked example (exercises the whole kernel)

```
// geometry.mdl — a Lamina source module

use math::{ sqrt };

const ORIGIN_X: f64 = 0.0;

@equatable
@displayable
struct Point {
    x: f64,
    y: f64,
}

enum Shape {
    Empty,
    Circle(f64),
    Rect { w: f64, h: f64 },
}

public fn area(s: Shape): f64 {
    switch (s) {
        case Circle(r) { return 3.14159 * r ** 2; }
        case Rect(rect) { return rect.w * rect.h; }
        default { return 0.0; }
    }
}

public fn distance(a: Point, b: Point): f64 {
    let dx: f64 = a.x - b.x;
    let dy: f64 = a.y - b.y;
    return sqrt(dx * dx + dy * dy);
}

public fn scale_all(points: [Point], factor: f64): f64 {
    let total = 0.0;
    foreach p in points {
        total = total + p.x * factor;
    }
    for (let i = 0; i < 10; i = i + 1) {
        total = total + 1.0;
    }
    let f = (x: f64): f64 => { return x * 2.0; };   // lambda (see note)
    return total;
}

// A tree-core document (e.g. an HTML fragment)
public fn render(): void {
    node div(class = "box") {
        node p {
            text "Hello"
        }
    }
}

// Native-interop escape hatch, portability cost visible per target
public fn fast_sqrt(x: f64): f64 {
    raw {
        rust   { return x.sqrt(); }
        python { return math.sqrt(x) }
        else   { return x; }
    }
}
```

## Open / deferred (NOT part of this spec)

- **Lambda surface spelling: RATIFIED** — `(params): ret => { body }`, with the
  return type OPTIONAL when inferable (`(x: i32) => { ... }`), since the kernel's
  `Expr::Lambda.return_type` is optional. Shown in the worked example.
- **Enum-payload binding in `switch` arms: RATIFIED (Option A)** —
  `case Circle(r) { ... }` binds a tuple payload positionally; `case Rect { w, h }
  { ... }` binds a struct payload by field name; the bound names are in scope
  within that arm's block. This REQUIRES a small, well-scoped KERNEL EXTENSION:
  a switch `case` must carry an optional payload-binding list (positional or
  named), and the emitter + every target def must render it (native pattern-match
  on Rust/Swift/Haskell/Kotlin; generated local-variable extraction from the
  tagged-union/sealed encodings on C/Go/etc.). This is its OWN implementation arc
  (kernel AST + emitter + all-16-def rendering + compiler-verified tests), done
  under the disclosed-not-hacked + independent-audit discipline — NOT folded
  silently into the parser work.
- **Rich SDK binding, dependency versioning, profiles' manifest mechanics** — all
  layer/tooling-arc concerns, deferred (see the layer-ecosystem wiki).
