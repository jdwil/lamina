# Lamina Language Definition Format

This is the canonical reference for authoring a Lamina **language definition** —
the `.mdl` document that tells the engine how to lower Lamina IR into one target
language. The engine ships with no built-in definitions; every target is an
external document in this format.

A language definition is *markdown-compatible* (it renders in any markdown tool)
but **strictly validated**: a missing or malformed load-bearing element is a
hard error, never silently ignored. Free-form prose between the load-bearing
elements is documentation for humans and is ignored by the parser — it should
describe *this target's* choices, not the format itself (that is what this
document is for).

## Document Shape

```text
# Lamina Language Definition: <name>

​```lang-meta
lamina-format: <min format version — semver>
target: <target language name>
target-version: <opaque target-language version band>
​```

<optional prose about this target>

## Function

​```template
<entry template — see Templates>
​```

### <slot>          (one subsection per non-terminal slot referenced above)
<a template block OR a When table>

## Passes                (OPTIONAL — see Passes and Output Regions)
​```lang-passes
passes: <ordered pass names>
regions: <output region names>
layout: <region concatenation order>
​```

## Capabilities
<a markdown table mapping each kernel primitive to an action>
```

## Title Line

The document MUST begin (first non-blank line) with:

```text
# Lamina Language Definition: <name>
```

The `<name>` (e.g. `rust`) is captured as the target's identity. Anything else
on the first line is an error.

## The `lang-meta` Header Block

Immediately near the title, every definition MUST carry a ```` ```lang-meta ````
fenced block declaring its two independent kinds of versioning. It is a
load-bearing, strictly-parsed block of `key: value` lines; all three keys are
**required** and are the only keys allowed:

```text
​```lang-meta
lamina-format: 0.0.0
target: rust
target-version: 2021
​```
```

- **`lamina-format`** — the **minimum** Lamina `.mdl` FORMAT version this
  definition requires, as a `major.minor.patch` semver. The engine carries a
  format-version constant (`LAMINA_FORMAT_VERSION`, currently `0.0.0`). On load
  the engine compares (a hand-rolled 3-integer compare — no semver dependency):
  if the declared minimum is **newer** than the engine's constant, the load is
  refused with `FormatVersionTooNew`; if **equal or older**, it loads (the engine
  is backward-compatible). A malformed version string is a load-time error, and a
  missing `lamina-format` is a load-time error — explicit is better.

- **`target`** — the target language name. Usually mirrors the title `<name>`,
  but is declared explicitly so the engine carries an authoritative language
  identity. Stored verbatim; the engine never parses it.

- **`target-version`** — the target-language version **band** this definition
  emits for (a sensible commonly-grouped band, e.g. Rust edition `2021`, `>=3.0`
  for Python, `>=5.0` for TypeScript, `html5`, `rfc8259`). This is an **opaque**
  string: the engine stores it verbatim on the parsed `LanguageDef` and **never**
  parses, compares, or branches on it. A future registry/resolution layer may
  filter candidate definitions by this band; the engine itself performs no
  version logic on it.

Missing block → load-time error; a non-`key: value` line, an empty value, an
unknown key, or a missing required key → load-time error.

### Version-specific behavior lives in SEPARATE definitions, not conditionals

The target-version band is a **selector/badge**, not a switch. Version-specific
behavior is expressed by shipping **distinct, self-contained definitions** — a
conservative `python` def (`target-version: >=3.0`) and a permissive
`python3.13` def (`target-version: >=3.13`, emitting newer syntax) are two
separate `.mdl` files with their own capability matrices and rendering. There is
deliberately **no** version-comparison predicate in the `When` language: the
engine must never branch on a target version, keeping it dumb and every target
concern in the (single-version) definition it belongs to.

## The Function Section

`## Function` MUST contain **exactly one** ```` ```template ```` fenced block —
the *entry template* — optionally preceded by prose. It is followed by any
number of `### <slot>` subsections.

### Templates

A template is text with `{slot}` holes. `{{` and `}}` are literal braces (so a
target whose syntax uses braces, like Rust's `{ ... }` body, is expressible).
Templates are authored at **column 0**; indentation is applied by the caller
(see Indentation).

A slot `{x}` is resolved by:
- an engine-provided **scalar terminal slot** — `name`, `ret_type` (function
  level), or `type`, `value` (within an item slot) — filled from the source AST
  / capability matrix; or
- an engine-provided **iterable terminal slot** — `params`, `body` — which loops
  the AST and renders an item slot per element (see Iterable Slots); or
- a same-named `### x` subsection, one heading level deeper.

Every referenced slot MUST resolve to one of the above, at every nesting depth.
A dangling slot reference is a **load-time** error.

### Iterable (Collection) Slots

`params` and `body` are **iterable** terminal slots: the engine loops the AST's
parameter list / statement list and renders a per-element **item slot** for each
element. The pairing is fixed:

| Collection slot | Item slot (required subsection) |
|-----------------|---------------------------------|
| `{params}`      | `### param`                     |
| `{body}`        | `### statement`                 |
| `{fields}` (in `struct_lit`) | `### field_init`   |

`{fields}` on a struct-literal (construction) expression loops the field
initializers, rendering `### field_init` per element; within it `{name}` is the
field name and `{value}` its initializer expression (a value position, so a
bare function name there is the function-pointer value form).

Referencing `{params}` without a `### param` subsection (or `{body}` without
`### statement`) is a load-time error.

**There is no separator field.** Each element renders itself *including any
separator*, deciding via the loop facts `first` and `last` (see When
Predicates). For example, a non-first parameter renders its own leading `, `:

```text
### param
| When  | Template |
|-------|----------|
| first | "{name}: {type}"   |
| else  | ", {name}: {type}" |
```

Within an item slot, `{name}` / `{type}` refer to the *element* (the param's
name/type); `{value}` refers to a statement's value. An empty collection renders
to the empty string.

### Slot Subsections

Each `### <slot>` is one of two forms:

1. **A fixed template** (non-branching) — a ```` ```template ```` block:

   ```text
   ### params
   ​```template
   self
   ​```
   ```

2. **A When table** (branching) — a markdown pipe table whose first column is a
   `When` predicate and second is a double-quoted template. Rows are evaluated
   top-to-bottom, first match wins, and the table MUST end with an `else` row:

   ```text
   ### ret
   | When         | Template |
   |--------------|----------|
   | ret is void  | ""              |
   | ret is never | " -> !"         |
   | else         | " -> {ret_type}" |
   ```

   Template cells are double-quoted; `\n`, `\t`, `\\`, and `\"` are unescaped.

### The `forbid` Directive

A Template-column cell is either a **double-quoted string** (text to render) or
the **unquoted bareword `forbid`** — the one allowed unquoted value. `forbid`
means the construct is not expressible in this target; when selected, emission
fails with a forbidden-construct error. Use it for a visibility level or
modifier the target lacks (e.g. `protected` in a language with no
module-visibility keyword):

```text
### vis
| When           | Template |
|-----------------|---------|
| vis is public   | "pub "  |
| vis is private  | ""      |
| else            | forbid  |
```

The rule is: **quoted = render this text; the bareword `forbid` = a directive.**
Any *other* unquoted text is an error (the strictness is otherwise preserved).
`forbid` here is the same lowercase `forbid` used as a capability action, so the
concept is spelled identically everywhere.

### Block-Slot References in Cells

A `When`-table Template cell has three possible forms:

- an **inline quoted template** (`"return {value};"`) — double quotes make
  significant whitespace explicit;
- the bareword **`forbid`** directive; or
- a bareword **section reference** `@name` — no quotes — which resolves to the
  same-named `### name` subsection one heading level deeper. That subsection may
  be a full multi-line ```` ```template ```` block.

A reference is written `@name` (not `"{name}"`): because a bare reference is just
a section name with no significant whitespace, it needs no quotes, and the `@`
distinguishes "go render this author-defined section" from an inline literal.
`@name` is exactly equivalent to a template of `{name}` — it reuses the same
slot-resolution and load-time validation (a dangling `@missing` is a load-time
error).

Because a cramped, `\n`-escaped multi-line cell and a reference to a clean
multi-line block slot render to the *same* string, prefer the `@name` reference
form for any branch whose output spans multiple lines. It keeps the `When` table
a scannable decision matrix (one readable row per case) while the multi-line body
lives in a named ```` ```template ```` block below, authored with real newlines
instead of `\n` escapes. One-liner arms (`"break;"`, `"return;"`, a bare
`"{value}"`) stay inline as quoted templates — the reference form earns its keep
only for multi-line output.

Note the distinction: `@name` references an **author-defined `###` section**;
`{name}` inside a template references a slot value (an engine-bound AST value, or
another slot). Only *cell-level* references use `@`; slots inside template bodies
always use `{}`.

For example, a statement-dispatch `while` arm reads as a one-line row:

```text
### stmt
| When         | Template |
|--------------|----------|
| stmt is while | @while_stmt |
| else          | forbid |

### while_stmt
​```template
while {cond} {{
    {body}
}}
​```
```

## When Predicates

The `When` column is a small, **closed** predicate language — not a scripting
language. Grammar:

```text
when      := "else" | or
or        := and ("||" and)*
and       := unary ("&&" unary)*
unary     := "!"? atom
atom      := fact | "(" or ")"
fact      := path ("is" IDENT | "eq" path)?
path      := IDENT ("." IDENT)?
```

A bare `IDENT` is a boolean fact; `IDENT is IDENT` is an enum query. A `path`
is a **one-level** sub-part reference (`value`, `value.op`, `target`) used by
the structural facts below — only one `.` is allowed, so deeper paths
(`value.lhs.rhs`) are a syntax error. The fact vocabulary is fixed by the engine
— a definition may only *use* facts, never invent them:

| Fact | Kind | Holds when |
|------|------|-----------|
| `export` | bool | the callable is publicly visible |
| `async` `const` `unsafe` `throws` `extern` `inline` `generator` | bool | the callable has that modifier |
| `ret is void` / `ret is never` / `ret is type` | enum | the return shape |
| `vis is public` / `vis is protected` / `vis is private` | enum | the visibility level |
| `caller is async` / `caller is sync` | enum | the enclosing callable's synchrony (inherited) |
| `first` / `last` | bool | this element is the first / last in the collection being looped (only meaningful inside an item slot) |
| `expr is <kind>` | enum | the expression being rendered is that kind (`int` `float` `bool` `string` `char` `null` `ref` `field` `index` `call` `unary` `binary` `cast` `struct_lit` `node` `text` `array` `raw` `lambda`) |
| `stmt is <kind>` | enum | the statement being rendered is that kind (`block` `let` `return` `if` `while` `for` `foreach` `switch` `break` `continue` `assign` `expr`) |
| `item is <kind>` | enum | the top-level item being rendered is that kind (`function` `struct` `enum` `typedef` `const` `use` `tree`) |
| `variant is <kind>` | enum | the enum variant being rendered has that payload shape (`unit` `tuple` `struct`) |
| `has_value` `has_type` `has_else` `has_init` `has_cond` `has_step` `has_default` | bool | the statement carries that optional sub-part |
| `has_len` | bool | (array type) the array being rendered carries an explicit length (sized `[T; N]` vs unsized `[T]`) |
| `has_items` | bool | (`use` import) the import carries a selective item list (`use path::{a, b}`) |
| `has_alias` | bool | (`use` import / import item) the module — or a selectively-imported item — carries an alias (`use path as p`, `a as b`) |
| `has_payload` | bool | (`enum`) at least one variant carries a payload (tuple or struct) — lets a target branch the whole enum to a discriminated-union form |
| `has_attributes` | bool | (`struct`/`enum`) the type carries at least one type-level attribute — lets a target branch its derive/annotation line |
| `has_ret_type` | bool | (`expr is lambda`) the lambda being rendered declares an explicit return type — lets a target render the return-type annotation conditionally |
| `attr is <name>` | enum | (inside a `### attribute` item slot) the type attribute being rendered is that one (`displayable` `equatable` `comparable` `hashable` `cloneable` `copyable` `hasdefault` `iterable`) |
| `value is <kind>` | enum | (one-level structural) the current node's direct `value` sub-part is that expression kind |
| `value.op is <op>` | enum | (one-level structural) the current node's `value` sub-part is a binary with that operator (machine name: `add` `sub` `mul` …) |
| `target eq value.lhs` | bool | (one-level structural) the current node's `target` sub-part is structurally equal to its `value` sub-part's left operand |
| `has_meta(<key>)` | bool | (metadata) the current node carries metadata key `<key>` — keys are open (a layer↔def contract), the *mechanism* is closed |
| `meta.<key> is <value>` | enum | (metadata) the current node's metadata `<key>` equals `<value>` — keys and values are open |
| `fnptr_ref_count(<arg>) is <n>` | enum | (helper) the function the `<arg>` sub-slot's fnptr value refers to is used as a value exactly `<n>` times in the unit |
| `type_of(<arg>) is <type>` | enum | (helper) the resolved type of the `<arg>` sub-slot's expression is `<type>` (a locally-nameable type) |
| `resolve(<arg>) is <kind>` | enum | (helper) the `<arg>` sub-slot's name resolves to a top-level item of that kind (`function` `struct` `enum` `typedef` `const`) |

### One-Level Structural Predicates (Idiom Recognition)

A language definition can recognize desugared idioms and emit their native form
using a **bounded, one-level** view of the current node's immediate sub-parts.
This is a purely *local* decision on the node being rendered — consistent with
the recursive model — and stays non-Turing-complete because the vocabulary is
closed: the engine defines which sub-part names, kinds, ops, and equality
pairings are answerable; a definition only *composes* them with `&&`/`||`/`!`.

Three structural query forms exist (all one level deep — no `a.b.c`):

1. **Sub-part kind:** `value is binary` — the [dispatch kind](#when-predicates)
   of a direct named sub-part (currently the `value` of an `assign`).
2. **Sub-part operator:** `value.op is add` — the binary operator (machine
   name) of the `value` sub-part, when it is a binary.
3. **Structural equality:** `target eq value.lhs` — whether two one-level
   sub-parts are structurally identical.

To *render* a matched sub-part, one-level sub-part **slots** are exposed too:
`{value.op}`, `{value.lhs}`, `{value.rhs}` resolve to the rendered sub-part
when the current node's `value` is a binary (same one-level rule). This lets a
target author compound assignment entirely in the definition — see the example
below. A target *without* `+=` simply omits the compound rows and falls through
to the plain `{target} = {value};` row: correct degradation. The same mechanism
recognizes ternary shape (an `if` whose branches are single values) and
increment (`i = i + 1` → `i++`).

```text
### stmt
| When                                                                        | Template |
|-----------------------------------------------------------------------------|----------|
| stmt is assign && value is binary && value.op is add && target eq value.lhs | "{target} += {value.rhs};" |
| stmt is assign                                                              | "{target} = {value};" |
```

### Assignment, Cast, and Construction Slots

- **Assignment** (`stmt is assign`) exposes `{target}` (the lvalue place —
  restricted to a ref/field/index at construction) and `{value}` (the assigned
  expression). Its `value` sub-part is also visible to the one-level structural
  facts/slots above (for the compound-assignment idiom).
- **Cast** (`expr is cast`) exposes `{value}` (the value being cast) and `{ty}`
  (the target type, rendered through the capability matrix). A target that
  cannot express the cast to a given type surfaces a forbidden-primitive error
  when `{ty}` is resolved. Typical spelling: `"{value} as {ty}"`.
- **Struct-literal construction** (`expr is struct_lit`) exposes `{type_name}`
  (the aggregate type name) and the `{fields}` collection (see `### field_init`
  above). A target chooses its spelling: Rust `Type { field: value }`,
  TypeScript a bare object literal `{ field: value }`.
- **Function-as-value (fnptr):** a bare function name used as a value is a
  function-pointer value (C-style decay) — there is no separate node; it is an
  `expr is ref` rendered as the plain identifier. Storing one into a
  `fnptr`-typed field is gated *transitively* by that field's declared `fnptr`
  type: a target whose capability matrix forbids `fnptr` cannot declare the
  field, so it cannot construct the aggregate either.

### Arrays, Enum Payloads, and Structured `use`

Three constructs complete the imperative kernel vocabulary. Richer/growable
collections (lists, maps, sets) are **layer** concerns, not kernel — only a
fixed array lives in the kernel.

- **Array type** (`### array`, resolved in type scope). Exposes `{elem}` (the
  element type, rendered recursively so nested arrays compose) and `{len}` (the
  textual length). The `has_len` fact picks the sized vs unsized form: Rust
  `[T; N]` / `[T]`, TypeScript `T[]` (which ignores the length). There is no
  `array` primitive; the `### array` slot **is** the capability escape hatch — a
  target with no array type forbids that slot.
- **Array literal** (`expr is array`). Exposes the `{elems}` collection (item
  slot `### array_elem`, with `first`/`last` loop facts for the comma
  separator). Both shipped targets spell it `[a, b, c]`. Indexing reuses the
  existing `expr is index` (`{obj}[{index}]`).
- **Enum payloads.** A variant dispatches on `variant is unit|tuple|struct` in
  its `### variant` item slot. A **unit** variant is a plain name; a **tuple**
  variant exposes the `{payload_types}` collection (item slot `### payload_type`,
  each element's `{type}` rendered through the type machinery); a **struct**
  variant exposes the `{payload_fields}` collection (item slot
  `### payload_field`, reusing the struct-field `{name}`/`{type}` sub-slots).
  Payloads are **capability-gated**: a C-style target with no tagged unions
  writes `forbid` in the tuple/struct rows of `### variant`, so a payload-bearing
  variant surfaces a forbidden-construct error while a unit variant still
  renders. A target with no native payload-carrying `enum` (TypeScript) branches
  the whole declaration on the enum-level `has_payload` fact to a discriminated
  union `type Name = … | …`. **Construction** of a payload variant reuses
  existing expressions — a tuple variant `Circle(5)` is an `expr is call`, a
  struct variant is an `expr is struct_lit` — so no new construction node exists.
- **Structured `use`.** The `## Use` entry branches on `has_items` (a selective
  list) and `has_alias` (a module alias): `use path;` (bare, byte-identical to
  before), `use path as p;` (alias), `use path::{a, b as c};` (selective).
  A selective import exposes the `{items}` collection (item slot `### use_item`,
  each with its own `has_alias` fact for the `name as alias` form) and `{alias}`;
  TypeScript spells the three forms `import path;`, `import * as p from path;`,
  `import { a, b as c } from path;`.

### Lambda (the functional-core primitive)

A **lambda** (`expr is lambda`) is the kernel's one first-class function value —
essentially an anonymous `Function`. It is the single irreducible functional
construct: `map`/`fold`/`filter`, ranges, list comprehensions, and
let-expressions all reduce to *lambda + recursion + application + collections*,
so they are **library** concerns (rendered idiomatically per target at the call
site via the def/metadata), not kernel. This one addition makes the kernel
dual-paradigm (imperative + functional).

A lambda exposes three slots (resolved in the lambda's own scope):

- `{params}` — the parameter collection, looped through the **shared
  `### param` item slot** (identical `{name}`/`{type}` sub-slots as a function
  parameter, each supplying its own `, ` separator via the `first` loop fact).
- `{ret_type}` — the optional declared return type, rendered through the type
  machinery. Guarded by the **`has_ret_type`** fact: an inference-only lambda
  omits it, an annotated lambda selects a row that renders it.
- `{body}` — the lambda body, a **statement block** (the general form) looped
  through the recursive `### statement` item slot, so a lambda body composes
  exactly like a function body. A single-expression lambda is simply a
  one-statement body.

**Capture is the target's concern** — the kernel adds no capture analysis.
Closure-capable targets render the node directly as their native closure. The
two shipped defs do so:

```text
### expr
| expr is lambda && has_ret_type | @lambda_ret |
| expr is lambda                 | @lambda     |
```

Rust spells it a closure `|params| { body }` (and `|params| -> Ret { body }`
when annotated); TypeScript spells it an arrow function `(params) => { body }`
(and `(params): Ret => { body }`). Both render the **block body form** for the
general case; this is the faithful rendering of the general statement-block body
and requires no additional kernel construct. A def *may* choose a concise
single-expression spelling for a one-statement body, but the shipped defs keep
the block form for uniformity.

Expression-only targets (Python: `lambda x: expr`) and lambda-less targets (C)
instead **realize** `Expr::Lambda` by the existing hoist-to-named-function path
(`origin=lambda` metadata + `fresh_name` + `{resolve_fnptr(...)}` inlining via
the `### anon_fn` slot). `Expr::Lambda` is the primary kernel representation of a
function value; that reconstruction path is how a target *without* native
closures realizes it — the two compose, they are not parallel lambda concepts.

### Type Attributes (derivable capabilities on `struct` / `enum`)

A `struct` or `enum` may request a set of **type-level attributes** — the
type-level analog of a callable modifier. The kernel reserves the *superset* of
behavioral capabilities a target may realize for an aggregate type:

`debug` · `eq` · `ord` · `hash` · `clone` · `copy` · `default` · `iterable`

Each attribute is **semantic metadata first, emitted text second** (like a
callable modifier): it stays attached to the IR node regardless of a target's
spelling, so it also informs the *raise* direction, examples, and analysis.
Attributes are **ignored by structural equality** (exactly as metadata is), so
requesting attributes never perturbs idiom recognition.

Attributes realize through the **item-slot mechanism**, not a separate matrix
section — consistent with how operators use a scalar slot rather than
per-operator facts. A `## Struct` / `## Enum` entry template references a
`{derive}` slot (or whatever the target names it) that branches on the
`has_attributes` fact, plus an `### attribute` item slot looped over the type's
attributes with the usual `first`/`last` loop facts and a per-element
`attr is <name>` dispatch fact. Each attribute has one of **three realization
outcomes**:

- **realize** — emit the target's derive/annotation text (Rust maps the whole
  set into one `#[derive(Debug, Clone, PartialEq)]` line before the keyword);
- **inherent** — emit the **empty string**, because the target provides the
  behavior with no declaration (the metadata-first principle: the attribute is
  not lost, it simply has no spelling);
- **forbid** — the **`forbid` sentinel**: the target cannot realize the
  attribute as a type attribute, so a type carrying it surfaces a
  forbidden-construct error and cannot target that language.

**Empty is byte-identical.** A type with no attributes leaves `has_attributes`
false, so `{derive}` renders `""` and the output matches the pre-attribute form
exactly.

The shipped `rust.mdl` folds the attributes into a single `#[derive(...)]` line
(`eq` → `PartialEq`, `ord` → `PartialOrd` — Rust's full `Ord` also needs
`Eq`/`PartialEq`, so the single idiomatic `PartialOrd` is used for this pass;
`iterable` → `forbid`, Rust having no single derive for iteration). The shipped
`typescript.mdl` has no derive mechanism, so its `### derive` slot renders `""`
when empty and `forbid`s when any attribute is present — a TS type that requests
attributes is a clean forbidden-construct while an attribute-free one stays
byte-identical.

## The Declarative Tree Core (`node` / `attr` / `text`)

Lamina has a **second kernel core** alongside the imperative one: a declarative
tree. It is the universal substrate of every document and structured-data format
— an HTML element, a CSS rule, a JSON object, a YAML/TOML table are all "a named
node with attributes and children." The kernel keeps the vocabulary generic and
frozen: only three nodes exist, and NO format-specific keyword (no `div`, no CSS
property) is kernel — those are language-definition detail or layer concerns.

A tree node is a **first-class expression**, so the two cores interoperate: a
`fn` may return a node (its return type is a `Named` type, e.g. `-> Node`,
reusing the ordinary type machinery), and a node's attribute value or child may
be an arbitrary expression, enabling JSX-like interpolation (`<div>{name}</div>`
is a `node` whose child is an `expr is ref`).

The three tree expression kinds dispatch through the SAME `### expr` table as
every other expression:

- **`expr is node`** — a named node. Exposes `{node_name}` (scalar leaf), the
  `{attrs}` collection (item slot `### attr`), and the `{children}` collection
  (item slot `### child`). A definition maps the generic node to its target
  syntax: HTML `<name attrs>children</name>`, JSON `{"tag": …, …}`.
- **`expr is text`** — literal text content. Exposes `{value}`, which renders
  its inner expression through the `### expr` dispatch (typically a string
  literal, but any expression for interpolation). HTML emits raw text; JSON
  emits a quoted string.
- **`### attr`** — the item slot for each element of a node's `{attrs}`. Exposes
  `{name}` (the attribute name) and `{value}` (its value expression, dispatched
  through `### expr`). Loop facts `first`/`last` let it supply its own
  separator, exactly like `### param` / `### expr_arg`.
- **`### child`** — the item slot for each element of a node's `{children}`.
  Exposes `{value}` (the child expression). Loop facts supply the separator (or
  none, as HTML uses).

Because a node is generic, the same tree AST renders to structurally-different
targets purely from the definition. From `div class="box"` containing
`p > "hi"`:

```text
### expr                       (HTML)
| When           | Template |
|-----------------|----------|
| expr is node    | @element |
| expr is text    | "{value}" |
| expr is string  | "{value}" |
| else            | forbid |

### element
```template
<{node_name}{attrs}>{children}</{node_name}>
```
```

emits `<div class="box"><p>hi</p></div>`, while a JSON definition of the same
`### expr` table (node → `{"tag": …}`, string → `"{value}"` quoted) emits a JSON
object from the identical AST.

### Top-level tree values (declarative-only files)

A file whose root **is** a tree — a pure markup/config document — is a top-level
`tree` item (`item is tree`). It has no `## <Item>` entry template of its own:
it renders straight through the shared `### expr` dispatch, exactly as an
embedded tree expression would.

A purely declarative target (HTML, JSON, YAML, …) has no imperative callable, so
it may **omit `## Function` entirely** and instead host its shared render slots
under a **`## Tree`** section. `## Tree` is slots-only (no entry template): it
holds `### expr` and its `### attr` / `### child` item slots (plus any node
sub-slots the target names), which are merged into the shared slot map and
validated at load time starting from `### expr`. `## Capabilities` is still
required (the matrix must cover every primitive); a definition must have at
least one of `## Function` or `## Tree`. See `lamina-defs/languages/html.mdl`
and `json.mdl` for complete declarative-only examples.

## The Raw / Verbatim Pass-Through Node (layer escape hatch)

Lamina provides an ultimate escape hatch: a **raw** node holds a string of
verbatim target code the engine emits **UNCHANGED**. It exists at all three
levels — an `expr is raw` expression fragment, a `stmt is raw` statement, and an
`item is raw` top-level item — so a *layer* can always produce the exact correct
output even when the kernel vocabulary plus the definition cannot express it.

The design is deliberately simple: a raw node is **not target-keyed**. Layers
lower differently per target, so a raw node only ever exists in the AST when the
layer lowered *for the current target* — by the time the engine sees it, it is
already correct target code by construction. The engine therefore does **no**
target check, variant selection, capability gating, or error path; it simply
passes the string through at the slot position. Do NOT add capability-matrix
gating for raw nodes — a raw node is, by definition, already correct.

A raw node also carries the standard [metadata](#construct-metadata) channel and
is **raiseable like any other node** (no special raise path); structural
equality compares the verbatim string and, as everywhere, ignores metadata.

**Rendering.** The verbatim string is exposed through the ordinary `value`
scalar slot, so a target needs only a trivial pass-through:

- **`### expr`** — add a row `| expr is raw | "{value}" |`.
- **`### statement`** — add a row `| stmt is raw | "{value}" |` (the raw string
  supplies its own terminator; the engine appends nothing).
- **Top-level items** dispatch to `## <Item>` sections, so add a **`## Raw`**
  section whose entry template is simply `{value}`:

  ```text
  ## Raw

  ​```template
  {value}
  ​```
  ```

  Like every `## <Item>` section, `## Raw` is **optional**: a definition that
  omits it simply cannot emit a raw item (using one becomes an emit-time
  `UnknownItem` error), so existing definitions are unaffected.

**Indentation of multi-line raw code.** A raw string is inserted at its slot
position and re-indented by the renderer's ordinary [column-derived
continuation-line rule](#indentation) — the SAME rule applied to every
multi-line rendered fragment: the leading whitespace of the current template
line is prepended to every line *after the first* of the raw string. So a
multi-line raw statement placed in a body whose `{body}` slot sits at column 4
has its second and later lines indented by 4 spaces; the first line's indent is
the literal template text preceding the slot; blank lines get no trailing
whitespace. The engine never re-flows, trims, or otherwise transforms the raw
text beyond this uniform continuation-line indentation. The shipped `rust.mdl`
and `typescript.mdl` both carry the three pass-through rows/section above.

## Construct Metadata

Every kernel construct carries an open, engine-**transparent** metadata channel:
an ordered key→value map (empty by default). It is how a *layer* communicates
intent about a construct to a *language definition* — e.g. "this hoisted `fn` is
a synthesized lambda" (`origin=anon_fn`), "this struct field is a captured
variable" (`capture=true`), "this struct-literal is a synthesized anonymous
class" (`origin=anon_class`).

The engine defines **no** keys, validates nothing, and has no opinion about any
key's meaning. There is **no** blessed metadata vocabulary — keys and values are
entirely a contract between a layer and a definition, documented by their
authors, never by the engine. (Blessing a key would make it a de-facto kernel
keyword, which the kernel refuses.) The engine only carries metadata through the
IR and answers two facts and one slot over it:

- **Facts** (`When` column): `has_meta(<key>)` (the node has that key) and
  `meta.<key> is <value>` (the node's `<key>` equals `<value>`). The *mechanism*
  is closed (only these two forms); the *keys and values* are open.
- **Slot** (template): `{meta.<key>}` renders the metadata value for `<key>`, or
  the empty string when absent (the forgiving contract).

Metadata **never participates in structural equality**: two constructs are
structurally equal iff their *structure* matches, regardless of metadata. The
`eq` predicate and all idiom matching ignore it, and a node with empty metadata
renders **byte-identically** to one built before metadata existed.

## Resolution / Render Helpers

A **closed** set of read-only, engine-provided helper functions lets a definition
cross the otherwise-local render boundary on demand — the rare cross-item need
(inline an anonymous function, count how often a function is used as a value,
resolve a field's type). Helpers are **pure** (never mutate the AST) and
**fixed**: a definition may only *call* the named helpers, never define new ones.
This is a controlled escape hatch, not a general query/scripting language — every
helper is a named, fixed-arity function over a per-unit symbol index the engine
builds once.

**Template-side** (produce rendered output, usable in templates/slots):

- `{resolve_fnptr(<arg>)}` — resolve the function the `<arg>` sub-slot's fnptr
  value refers to and render it **inline** as the target's anonymous-function
  spelling. If the definition provides a `### anon_fn` slot, the resolved
  function renders through it (giving the def control of the closure/arrow form,
  with access to the function's `{params}`/`{body}`/`{ret_type}`); otherwise the
  full `## Function` declaration is inlined.
- `{escape(<arg>, <style>)}` — escape the `<arg>` sub-slot's string/char literal
  contents for the target. `<style>` is a closed set: `c` (C-family: `\n \t \\ \"
  \' \r`), `json` (no `\'`), or `raw` (verbatim). The template supplies the
  surrounding quotes; the helper escapes the (unescaped) stored contents.
- `{field_type(<struct>, <field>)}` — render the declared type of `<field>` on
  the struct named `<struct>` (literal names), through the target's type
  machinery. For annotating/casting reconstructed field bindings.

**Predicate-side** (produce facts, usable in `When`):

- `fnptr_ref_count(<arg>) is <n>` — how many times the referenced function is
  used as a *value* in the unit (a fnptr reference, not a direct call). Drives an
  inlining policy: inline when `1`.
- `type_of(<arg>) is <type>` — the resolved type of the `<arg>` expression, where
  it is locally nameable (a struct-literal's aggregate type, a const's declared
  type). Used to identify e.g. that a struct-literal's type is a layer-synthesized
  anonymous class.
- `resolve(<arg>) is <kind>` — resolve a name to its top-level item kind
  (`function`/`struct`/…). Used with the resolved node's metadata to reconstruct
  higher-level forms.

An `<arg>` is an engine sub-slot of the current node (e.g. `value` in a
`### field_init` row, or `self` for the node itself). An unknown escape `<style>`,
a non-string `escape` argument, or a `resolve_fnptr` argument that is not a
resolvable function reference is a loud error, never silent wrong output.

## Anonymous-Form Reconstruction

Anonymous functions, closures, and anonymous classes are **not** kernel
constructs — the kernel stays `struct` + `fn` + `fnptr` + struct-literal, exactly
as with OOP. A *layer* lowers each form into that minimal shape and attaches
metadata; the *language definition* reconstructs the idiomatic form from the
shape + metadata + helpers. A target with no inline form (e.g. C) simply omits
the reconstruction rows and emits the lowered shape directly — correct
degradation.

- **Anonymous function** — a hoisted `fn` (tagged e.g. `origin=anon_fn`) plus a
  fnptr reference at the use-site. The def, where the fnptr value appears (a
  struct-literal field, a call argument), branches on
  `resolve(value) is function && fnptr_ref_count(value) is 1` and inlines it with
  `{resolve_fnptr(value)}` — rendered through `### anon_fn` as a closure
  (`|x| …`) or arrow (`x => …`).
- **Closure** — a capture-environment `struct` (fields tagged e.g. `capture=true`,
  the struct tagged e.g. `role=closure_env`) plus a hoisted `fn` taking the
  environment (tagged e.g. `origin=lambda`) plus a struct-literal constructing the
  environment. The def reads the **metadata** — not any structural convention —
  to emit a native closure capturing exactly the tagged fields.
- **Anonymous class** — an (anonymous) `struct` with `fnptr` method fields plus
  hoisted method `fn`s plus a struct-literal instantiating it (tagged e.g.
  `origin=anon_class`, method fields tagged e.g. `member=method`). The def
  recognizes the tagged struct-literal (`meta.origin is anon_class`), renders it as
  the target's object/anonymous-class expression, and inlines each method via
  `{resolve_fnptr(value)}`. A target with no object expression (Rust) keeps the
  named struct-literal.

The shipped `rust.mdl` reconstructs the anonymous *function* as a closure (and
keeps a named aggregate for the anonymous class, Rust having no object
expression); `typescript.mdl` reconstructs the anonymous function as an arrow and
the anonymous class as an object literal with inlined methods.

## Passes and Output Regions

Some target output cannot be produced by a single in-place render pass: lowering
an imperative `while` to a functional recursive helper needs the helper
**hoisted** to the top *and* a call left inline; a C-style declare-at-top layout
needs locals collected separately from the body; imports/preamble collection
needs a place to accumulate. Lamina expresses these entirely in the definition
via two DUMB engine primitives — **multi-pass rendering** and **named output
regions** — with the engine attaching NO meaning to any pass or region name.

The engine gains exactly two abilities: (1) it renders the whole unit once per
declared pass, in declared order, and (2) it maintains a set of author-named
output buffers (regions) and assembles them into the final output per a declared
layout. Everything else — what a pass *does*, what a region *means* — lives in
the definition's rule annotations. A definition that declares **no** `## Passes`
section behaves exactly as before: a single implicit pass, inline output,
byte-identical to the pre-passes emitter.

### The `## Passes` Section

An OPTIONAL `## Passes` section carries a single ```` ```lang-passes ````
fenced block with exactly three required `key: value` lines, each a
comma-separated list of author-named identifiers:

```text
## Passes

​```lang-passes
passes: collect, emit
regions: helpers, body
layout: helpers, body
​```
```

- **`passes`** — the ordered passes. The engine renders the whole unit once per
  pass, in this order, setting a *current pass* the rule annotations key off.
- **`regions`** — the author-named output buffers. A conventional region named
  `body` receives inline/unrouted emission; other regions receive rule output
  explicitly routed to them.
- **`layout`** — the order the regions concatenate into the final output. Every
  layout entry MUST be a declared region; `body` may appear to position the
  inline output relative to routed regions.

A missing/duplicate/unknown key, an empty list, or a `layout` naming an
undeclared region is a load-time error. The engine never parses or branches on
the names — they are opaque identifiers.

### Scoping Rules to a Pass / Region

A slot subsection or a `When`-table **row** may carry two OPTIONAL annotations,
reusing the existing declarative style:

- **`pass: <name>`** — the rule is active ONLY during that pass. Absent means
  active in every pass.
- **`region: <name>`** — the rule's rendered output is routed into that region's
  buffer instead of the inline/default output. Absent means inline (which, in
  multi-pass mode, accumulates into the conventional `body` region).

On a **`When`-table row**, annotations are extra trailing pipe cells:

```text
### statement
| When          | Template      | (annotations)                  |
|---------------|---------------|--------------------------------|
| stmt is while | @while_helper | pass: collect | region: helpers |
| else          | ""            | pass: collect |                |
| stmt is while | @while_call   | pass: emit    |                |
| else          | "{stmt}"      | pass: emit    |                |
```

Rows whose `pass:` does not match the current pass are skipped during selection,
so a table's unannotated `else` row still catches every pass. On a **fixed
`template` slot**, the annotation is a `pass:` / `region:` line placed right
after the `### <slot>` heading, before the `template` block:

```text
### pre
region: helpers
​```template
// preamble
​```
```

(A slot-level annotation is only meaningful on a fixed `template` slot; annotate
individual rows of a `When` table instead.) An annotation naming a pass/region
the `## Passes` section never declared — or ANY annotation in a definition with
no `## Passes` section — is a load-time error.

### The `fresh_name(prefix, key)` Helper

To coordinate a hoisted definition and its inline reference across passes and
regions, the engine provides a template-side helper `{fresh_name(<prefix>,
<key>)}`. It returns a unit-stable unique identifier, **memoized by
`(prefix, key)`**: the SAME `(prefix, key)` always renders to the SAME name for
the whole unit, no matter which pass or region requests it. This is what lets a
`while` rule emit `fn {fresh_name(loop, w)}() { … }` into the `helpers` region
(during one pass) and `{fresh_name(loop, w)}();` inline (during another) with
both agreeing on, say, `loop_0`. The generated form is `<prefix>_<n>` where `n`
is a monotone per-unit counter, so distinct `(prefix, key)` pairs never collide.
`fresh_name` is a closed helper (only these names exist), called exactly like
the other render helpers.

### Assembly

After all passes run, the engine concatenates the region buffers in `layout`
order to form the final output. A layout region that received no emission
contributes the empty string. Because the whole mechanism is off when no
`## Passes` section is present, adding it to a definition is purely additive:
existing definitions are unaffected and their output is byte-identical.

## Capabilities

`## Capabilities` is a markdown table mapping each kernel primitive to exactly
one **action**. Columns: `Primitive | Action | Target [| Notes]`. The Notes
column is ignored. A target is required for every action except `forbid`.

The matrix MUST be **complete**: every one of the 26 kernel primitives must have
a row. A missing primitive is a load-time error listing what is absent. Use
`forbid` for primitives the target cannot express (e.g. `ptr` in a language with
no raw pointers).

| Action | Meaning |
|--------|---------|
| `identity` | same type, same width/range/meaning (`i32` → `i32`) |
| `alias` | same values, different name, no range change (`void` → `()`) |
| `widen` | target represents every kernel value and more (`i8` → `int`) |
| `wrap` | target has no matching primitive; a named stand-in type (`bytes` → `Vec<u8>`) |
| `forbid` | not expressible in this target; use is an error |

## Indentation

Templates are written at column 0. When a slot appears indented within a
template, the renderer prepends that indentation to **every line after the
first** of the slot's rendered value (the first line's indent is the literal
text preceding the slot). The prepended amount is the **indentation of the
current line** — the width of its leading whitespace — not the full character
column, so a multi-line slot placed *after* non-whitespace text on a line stays
aligned to that line's indent rather than being pushed out by the preceding
text. This is what lets a brace-language `else` arm authored as
`}} else {else}` render its block left-aligned to the `if`. Blank lines get no
trailing whitespace. Because this threads through the recursive renderer, nested
indentation accumulates naturally — a template never hard-codes the indentation
of its container.

### Statement clauses (`init_clause` / `step_clause`)

A C-style counted `for (init; cond; step)` header composes an initializer and a
step as **clauses**, not statements: the header's own `;` separators are the
only terminators, so the init/step MUST NOT carry a statement terminator of
their own. A target that emits such a header therefore renders `for`'s init/step
through the `{init_clause}` / `{step_clause}` sub-slots, which dispatch to a
`### stmt_clause` `When` table (the terminator-free sibling of `### statement`,
sharing the same `stmt is <kind>` dispatch). A target that instead desugars the
counted loop (e.g. Rust, which has no C-style `for`) uses the full-statement
`{init}` / `{step}` sub-slots, where the trailing terminator is correct. Only
the `let` and `expr` clause kinds are reachable from a `for` header.

## Prose

Prose outside the load-bearing elements is ignored by the parser but is the
human-facing documentation of the artifact (and future *raise* input). It SHOULD
describe *this target's* implementation choices where useful (e.g. "TypeScript
has a single `number` type, so integers widen"), and SHOULD NOT restate this
format specification.
