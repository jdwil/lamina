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

A quoted Template cell is itself a template, so it may consist of nothing but a
single slot reference — `"{slot}"` — which resolves like any other slot: to an
engine terminal or to a same-named `### slot` subsection one heading level
deeper. That referenced subsection may be a full multi-line ```` ```template ````
block. So a `When` cell has three possible forms:

- an **inline quoted template** (`"return {value};"`),
- the bareword **`forbid`** directive, or
- a **reference** `"{slot}"` to another slot — possibly a multi-line block.

Because a cramped, `\n`-escaped multi-line cell and a reference to a clean
multi-line block slot render to the *same* string, prefer the reference form for
any branch whose output spans multiple lines. It keeps the `When` table a
scannable decision matrix (one readable row per case) while the multi-line body
lives in a named ```` ```template ```` block below, authored with real newlines
instead of `\n` escapes. A cell placed at the start of its template introduces
no indentation of its own, so the block renders byte-identically to the inline
form it replaces. One-liner arms (`"break;"`, `"return;"`, a bare `"{value}"`)
stay inline — the reference form earns its keep only for multi-line output.

For example, a statement-dispatch `while` arm reads as a one-line row:

```text
### stmt
| When         | Template |
|--------------|----------|
| stmt is while | "{while_stmt}" |
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
fact      := IDENT ("is" IDENT)?
```

A bare `IDENT` is a boolean fact; `IDENT is IDENT` is an enum query. The fact
vocabulary is fixed by the engine — a definition may only *use* facts, never
invent them:

| Fact | Kind | Holds when |
|------|------|-----------|
| `export` | bool | the callable is publicly visible |
| `async` `const` `unsafe` `throws` `extern` `inline` `generator` | bool | the callable has that modifier |
| `ret is void` / `ret is never` / `ret is type` | enum | the return shape |
| `vis is public` / `vis is protected` / `vis is private` | enum | the visibility level |
| `caller is async` / `caller is sync` | enum | the enclosing callable's synchrony (inherited) |
| `first` / `last` | bool | this element is the first / last in the collection being looped (only meaningful inside an item slot) |

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
