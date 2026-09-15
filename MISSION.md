# Lamina Mission

Lamina is an intermediate representation language and runtime whose purpose is to provide flexible and configurable grammars that lower to native source code and raise to English description, executable examples, and visual projections a human can review and sign off.

Licensed **AGPL-3.0-only** (JD Williams, jd@unsung-operators.com). Hosted
ProductHost and commercial licenses: same address.

## Engine

The engine has a finite set of primitives and keywords it understands. In order for the engine to produce raw code during transpilation, it must be provided a language file. Each language file provides a capability matrix, which lets the engine know which of its primitives and keywords the target language supports. Layers may only lower to Lamina code utilizing constructs that are supported by the capability matrix of the given language file. This means a given Lamina project may transpile to multiple languages, but not all languages. Here is a list of keywords and primitives that are supported.

### Primitives

i8, i16, i32, i64, i128, u8, u16, u32, u64, u128, isize, usize, f16, bf16, f32, f64, f128, bool, void, never, byte, bytes, char, str, ptr, fnptr

### Keywords

file, use, fn, struct, enum, typedef, const, if, else, switch, case, default, while, for, return, break, continue, true, false, null

### Capability Matrix

Capability matrices apply primarily to language primitives/types. Keywords are kernel syntax. Control-flow keywords may be desugared by the engine into a smaller statement set the language file must implement (fn, struct, if, while, return, block). Eg, a language file does not get to forbid switch. It may only forbid primitives, and null follows ptr.

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
