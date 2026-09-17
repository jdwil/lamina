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
| `expr is <kind>` | enum | the expression being rendered is that kind (`int` `float` `bool` `string` `char` `null` `ref` `field` `index` `call` `unary` `binary` `cast` `struct_lit`) |
| `stmt is <kind>` | enum | the statement being rendered is that kind (`block` `let` `return` `if` `while` `for` `foreach` `switch` `break` `continue` `assign` `expr`) |
| `item is <kind>` | enum | the top-level item being rendered is that kind (`function` `struct` `enum` `typedef` `const` `use`) |
| `has_value` `has_type` `has_else` `has_init` `has_cond` `has_step` `has_default` | bool | the statement carries that optional sub-part |
| `value is <kind>` | enum | (one-level structural) the current node's direct `value` sub-part is that expression kind |
| `value.op is <op>` | enum | (one-level structural) the current node's `value` sub-part is a binary with that operator (machine name: `add` `sub` `mul` …) |
| `target eq value.lhs` | bool | (one-level structural) the current node's `target` sub-part is structurally equal to its `value` sub-part's left operand |

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
