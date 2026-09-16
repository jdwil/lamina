# Lamina Mission

Lamina is an intermediate representation language and runtime whose purpose is to provide flexible and configurable grammars that lower to native source code and raise to English description, executable examples, and visual projections a human can review and sign off.

Lamina describes **projects, not just programs**. A project is the union of imperative source code (backend and frontend logic), declarative documents (markup and styles), structured configuration (JSON, YAML, TOML, framework manifests, tailwind configs, and the like), and the file-tree layout that arranges them. A single Lamina project may therefore lower to a whole framework's worth of artifacts, not one file.

Licensed **AGPL-3.0-only** (JD Williams, jd@unsung-operators.com). Hosted
ProductHost and commercial licenses: same address.

## Engine

The engine has a finite set of primitives and keywords it understands. In order for the engine to produce raw code during transpilation, it must be provided a language file. Each language file provides a capability matrix, which lets the engine know which of its primitives and keywords the target language supports. Layers may only lower to Lamina code utilizing constructs that are supported by the capability matrix of the given language file. This means a given Lamina project may transpile to multiple languages, but not all languages. Here is a list of keywords and primitives that are supported.

### Two Kernel Cores

The kernel spans two small, orthogonal paradigms. Most languages use one; some use both.

- **Imperative core** — statements, expressions, and functions. Describes *behavior*. This is the `fn`/`if`/`while`/`return` world and the numeric/scalar primitives below. Targets: Rust, TypeScript, Swift, Kotlin, Python, etc.
- **Declarative tree core** — named tree nodes with attributes and text. Describes *structure*. This is the substrate shared by **all** document and structured-data formats: an HTML element, a CSS rule, a Markdown block, a JSON object, a YAML/TOML table are all "a named node with attributes and children." Targets: HTML, CSS, Markdown, XML, JSON, YAML, TOML, config formats.

Both cores stay deliberately tiny and frozen. Concrete vocabularies — every HTML tag, every CSS property, a specific config schema, a framework component model — are NOT kernel; they are language-definition detail or, better, **layers** built over the two cores. (The exact tree-core keyword set is intentionally left unpinned until the tree-core slice is built and can be pressure-tested against real HTML/CSS/JSON examples.)

A project also has a dimension neither core alone captures: the **file tree** itself — which files exist, their paths, and their formats. A directory tree is itself a named tree (a directory is a node; a file is a leaf whose content is imperative source or a tree document), so it fits the tree-core concept, but it introduces a new output model: the engine emits a *filesystem layout of many artifacts*, not a single string. This is where `lamina.toml` and composite/meta language files live.

### Primitives

i8, i16, i32, i64, i128, u8, u16, u32, u64, u128, isize, usize, f16, bf16, f32, f64, f128, bool, void, never, byte, bytes, char, str, ptr, fnptr

### Keywords

Keywords fall into two classes.

**Structural keywords** are kernel syntax that every target must be able to express. They are never gated by the capability matrix and may be desugared by the engine into a smaller statement set the language file must implement (fn, struct, if, while, return, block):

file, use, fn, struct, enum, typedef, let, if, else, switch, case, default, while, for, foreach, return, break, continue, true, false, null

(`let` is the local-binding statement; `foreach` is the iterator loop, distinct from the C-style counted `for` — see the imperative-core statement set.)

**Capability keywords** are effects/attributes that not every target supports. They ARE gated by the capability matrix (a language file may forbid them; forbidding one means a unit that uses it cannot target that language). See *Callable Modifiers* below:

const, async, unsafe, throws, extern, inline, generator

### Callable Modifiers

The kernel reserves the **superset** of modifiers that can be applied to a callable in *any* target language. The kernel is permissive; each language file is the filter — it declares how to spell each modifier or forbids the ones its target lacks. The engine is dumb about them: it reads which modifiers a callable carries, asks the language file for each one's spelling (or errors if forbidden), and places the text via a template slot. All *behavior* (e.g. how async actually works, error propagation for throws) lives in layers or the target's own runtime, never the engine.

A modifier is **semantic metadata first, emitted text second.** Some modifiers usually produce declaration text (`async`, `const`); some may emit nothing in a given target (e.g. `generator` in Python, where the sequence nature is expressed by `yield` in the body) yet still remain attached to the IR node as metadata that informs the *raise* direction, examples, layers, and analysis. The language file's slot may therefore be an empty string; the modifier is not thereby lost.

**On/off modifiers** (a callable either has each or not): `const`, `async`, `unsafe`, `throws`, `extern`, `inline`, `generator`. Each is a forbiddable capability keyword with a language-file slot for its spelling and a position in the `decl` template (Rust prefix `async `, Kotlin `suspend `, Swift postfix ` async` — spelling from the matrix, position from the template).

**Visibility** is not on/off but a choice among three kernel levels: `public`, `protected`, `private`. Each language file maps each kernel level to its target spelling or forbids it (Rust `pub` / `pub(crate)` / none; Swift `public` / `internal` / `private`; TypeScript may forbid `protected` for free functions). For free functions in kernel v0 there is no inheritance, so `protected` reads as the middle tier (more than private, less than public) and maps to a target's module/package visibility. (When an OOP layer later introduces classes, an OOP-level `protected` with true subclass semantics will be reconciled against this kernel module-tier meaning.)

Modifiers that apply only to a *method on a type* (static, override, virtual, abstract, final, mutating, class-vs-instance) are NOT kernel callable modifiers — methods and receivers are layer concepts, so those belong to an OOP layer, not the kernel.

### Operators

The kernel reserves a **finite, closed superset** of semantically-distinct primitive operators. Like primitives and callable modifiers, operators are gated per target: a language file may `forbid` an operator its target lacks, or map its spelling. Truly exotic or sugar operators (matrix-multiply `@`, ranges `..`/`...`, try `?`, optional-chaining `?.`, the comma operator) are **NOT kernel operators** — they are layer concerns built over the primitives below.

**Unary:** `-` (negate), `!` (logical not), `~` (bitwise not), `+` (unary plus).

**Binary:**
- Arithmetic: `+` `-` `*` `/` `%` (remainder), `**` (power), `//` (floor-div).
- Comparison: `==` `!=` `<` `<=` `>` `>=`.
- Logical: `&&` `||`.
- Bitwise: `&` `|` `^` `<<` `>>` `>>>` (unsigned/logical right shift).

A target that lacks an operator (e.g. no `>>>`, no `**`) `forbid`s it or the language file maps it to a call/library form (which, if it is really a library function, is a layer concern rather than an operator mapping). Operator precedence and associativity are a *parsing* concern (the AST is already a tree); the emitter parenthesizes to preserve grouping.

### Capability Matrix

Capability matrices apply to language primitives/types, to capability keywords (the callable modifiers above), and to operators (see *Operators*). Structural keywords are kernel syntax and are never gated — a language file does not get to forbid `switch` (it desugars). It may forbid primitives, capability keywords, and operators, and `null` follows `ptr`. Control-flow keywords may be desugared by the engine into a smaller statement set the language file must implement (fn, struct, if, while, return, block).

Each kernel primitive maps to exactly one of five **actions**. The target type is required for every action except `forbid`. These five are the whole vocabulary — there is no `narrow` or `reinterpret`; a conversion that changes meaning is a cast written in Lamina, not something the matrix does on the way out.

- **identity** — the kernel primitive and the target type are the same type with the same width, range, and meaning (`i32 → i32`). No conversion is inserted; check does not warn.
- **alias** — the same values under a different name; no range change (`void → ()`, `byte → u8`, `f64 → Python float`). Lowering only rewrites the type spelling; check does not warn.
- **widen** — the target type can represent every kernel value and more (`i8 → Python int`). Lowering may insert a conversion; check records a width diagnostic so a later narrow (storing back into `i8`) must be explicit. Never silently truncates.
- **wrap** — the target has no matching primitive, so the language file supplies a named stand-in type (`bytes → Vec<u8>`, `str → String`). The value is an object that *stands in for* the primitive, not the primitive itself; check treats operations as whatever the wrapper declares.
- **forbid** — the primitive cannot appear in a unit targeting this language (`ptr` on Python). Check fails. No substitute, no emit. A concept the kernel does not have (generics, async, receivers) must be `forbid` on targets that happen to support it, so codegen cannot invent it.

#### Capability Matrix Example (Python)

```
i8..i128 u8..u128 isize usize   -> widen  int
f32 f64                         -> alias  float
f16 bf16 f128                   -> forbid
bool void never                 -> identity
byte                            -> widen  int
bytes                           -> wrap   bytes
str                             -> wrap   str
char                            -> alias  str
ptr fnptr                       -> forbid
```

### Language File Format

A language file is a rigid, markdown-compatible `.mdl` document. Target syntax is expressed as **templates with named slots**, not a fixed set of flags — because flags cannot describe the real variation (`-> !` for never vs. omitted `()` for void vs. `-> i32` for a normal return are three spellings of one return slot). One `decl` template plus slot variants (`ret`, `ret_void`, `ret_never`, `param`, `export_on`, …) covers it; a different target fills the same slots with different spellings (this is exactly how an indentation-based target like Python is supported). Alongside the template sits a small **policy** table (identifier style, statement terminator, empty-body spelling, etc.). When a new construct is added (`if`, `return`), it follows the same pattern: one template plus a tiny policy table, never a new mini-language per construct.
