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

<optional prose about this target>

## Function

​```template
<entry template — see Templates>
​```

### <slot>          (one subsection per non-terminal slot referenced above)
<a template block OR a When table>

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
| `expr is <kind>` | enum | the expression being rendered is that kind (`int` `float` `bool` `string` `char` `null` `ref` `field` `index` `call` `unary` `binary` `cast` `struct_lit` `node` `text` `array`) |
| `stmt is <kind>` | enum | the statement being rendered is that kind (`block` `let` `return` `if` `while` `for` `foreach` `switch` `break` `continue` `assign` `expr`) |
| `item is <kind>` | enum | the top-level item being rendered is that kind (`function` `struct` `enum` `typedef` `const` `use` `tree`) |
| `variant is <kind>` | enum | the enum variant being rendered has that payload shape (`unit` `tuple` `struct`) |
| `has_value` `has_type` `has_else` `has_init` `has_cond` `has_step` `has_default` | bool | the statement carries that optional sub-part |
| `has_len` | bool | (array type) the array being rendered carries an explicit length (sized `[T; N]` vs unsized `[T]`) |
| `has_items` | bool | (`use` import) the import carries a selective item list (`use path::{a, b}`) |
| `has_alias` | bool | (`use` import / import item) the module — or a selectively-imported item — carries an alias (`use path as p`, `a as b`) |
| `has_payload` | bool | (`enum`) at least one variant carries a payload (tuple or struct) — lets a target branch the whole enum to a discriminated-union form |
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
