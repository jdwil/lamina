//! Tests for the markdown front-end of the source parser.
//!
//! A source `.mdl` is a markdown document whose Lamina code lives in
//! ```` ```lamina ```` fenced blocks. [`parse`] extracts every such block,
//! concatenates their bodies in document order into ONE compilation unit, then
//! lexes+parses that. These tests prove:
//!
//! - a single-block document parses to the expected [`File`];
//! - a MULTI-block document (a `struct` under one `##` heading, a `fn` under
//!   another, with prose and an other-language fence between) concatenates to a
//!   `File` STRUCTURALLY EQUAL to the equivalent all-in-one-block document —
//!   proving document-order concatenation and cross-block references;
//! - prose, headings, and non-`lamina` fenced blocks between the code blocks are
//!   ignored;
//! - a document with NO ```` ```lamina ```` block parses to an empty `File`.

use lamina_core::ast::{
    Expr, Field, Function, Item, Meta, Primitive, Statement, Type, Visibility,
};
use lamina_core::parser::parse;

/// A single ```` ```lamina ```` block parses to the function it contains.
#[test]
fn single_block_document_parses() {
    let doc = "\
# A tiny module

Some prose introducing the function.

```lamina
public fn answer(): i32 {
    return 42;
}
```
";
    let file = parse(doc).expect("single-block document parses");
    assert_eq!(
        file,
        lamina_core::ast::File {
            items: vec![Item::Function(Function {
                name: "answer".to_string(),
                visibility: Visibility::Public,
                modifiers: vec![],
                params: vec![],
                return_type: Type::Primitive(Primitive::I32),
                body: vec![Statement::Return(Some(Expr::IntLiteral("42".to_string())))],
                meta: Meta::new(),
            })],
        }
    );
}

/// A document whose single block has a leading-space-indented fence line still
/// opens/closes the block (fences are matched on the TRIMMED line), and the
/// body is captured verbatim at its authored columns.
#[test]
fn indented_fence_lines_are_recognized() {
    let doc = "\
## Heading

   ```lamina
fn f(): i32 { return 1; }
   ```
";
    let file = parse(doc).expect("indented fences recognized");
    assert_eq!(file.items.len(), 1);
}

/// The motivating case: a `struct` in one block under `## Point`, a `fn` that
/// REFERENCES that struct in a second block under `## translate`, with prose and
/// an other-language (```rust```) fence between — concatenates in document order
/// to a File STRUCTURALLY EQUAL to the same code in one block. This proves both
/// document-order concatenation and cross-block references (the `fn`'s parameter
/// type names the struct declared in the earlier block).
#[test]
fn multi_block_concatenation_equals_single_block() {
    let multi = "\
# Geometry

A point type, then an operation over it, documented across two blocks.

## Point

The aggregate the module operates on.

```lamina
struct Point {
    x: i32,
    y: i32,
}
```

Here is an illustrative snippet in a DIFFERENT language that MUST be ignored:

```rust
let p = Point { x: 0, y: 0 }; // not Lamina — ignored
```

## translate

Shifts a point. Note it references `Point` declared in the block above —
proving the two blocks form ONE compilation unit.

```lamina
public fn translate(p: Point, dx: i32): Point {
    return Point { x: p.x + dx, y: p.y };
}
```
";

    let single = "\
# Geometry (single block)

```lamina
struct Point {
    x: i32,
    y: i32,
}

public fn translate(p: Point, dx: i32): Point {
    return Point { x: p.x + dx, y: p.y };
}
```
";

    let from_multi = parse(multi).expect("multi-block document parses");
    let from_single = parse(single).expect("single-block document parses");

    // Structural equality (the AST derives PartialEq) proves the two documents
    // produce the identical compilation unit.
    assert_eq!(from_multi, from_single);

    // Sanity: it is the expected two-item program (struct then fn), confirming
    // document order is preserved (not reordered or deduplicated).
    assert_eq!(from_multi.items.len(), 2);
    match &from_multi.items[0] {
        Item::Struct { name, fields, .. } => {
            assert_eq!(name, "Point");
            assert_eq!(
                *fields,
                vec![
                    Field {
                        name: "x".to_string(),
                        ty: Type::Primitive(Primitive::I32),
                        visibility: Visibility::Public,
                        meta: Meta::new(),
                    },
                    Field {
                        name: "y".to_string(),
                        ty: Type::Primitive(Primitive::I32),
                        visibility: Visibility::Public,
                        meta: Meta::new(),
                    },
                ]
            );
        }
        other => panic!("expected a struct first, got {other:?}"),
    }
    match &from_multi.items[1] {
        Item::Function(f) => {
            assert_eq!(f.name, "translate");
            // The parameter type names the struct from the FIRST block.
            assert_eq!(f.params[0].ty, Type::Named("Point".to_string()));
        }
        other => panic!("expected a function second, got {other:?}"),
    }
}

/// Three blocks concatenate in order, and a later block may use a `const`
/// declared in an earlier one (cross-block reference through a third block).
#[test]
fn three_blocks_concatenate_in_document_order() {
    let doc = "\
# Three

```lamina
const BASE: i32 = 10;
```

prose

```lamina
struct S { v: i32 }
```

more prose

```lamina
fn use_both(): i32 { return BASE; }
```
";
    let file = parse(doc).expect("three-block document parses");
    assert_eq!(file.items.len(), 3);
    assert!(matches!(file.items[0], Item::Const { .. }));
    assert!(matches!(file.items[1], Item::Struct { .. }));
    assert!(matches!(file.items[2], Item::Function(_)));
}

/// A document with NO ```` ```lamina ```` block parses to an EMPTY `File` (no
/// items) — the documented behavior (an undocumented / not-yet-written source
/// document is empty, not an error).
#[test]
fn document_with_no_lamina_block_is_empty_file() {
    let doc = "\
# Just prose

No code here yet.

```rust
fn main() {}
```

Still nothing in Lamina.
";
    let file = parse(doc).expect("prose-only document parses to empty File");
    assert!(file.items.is_empty());
}

/// The fully empty string is likewise an empty `File`.
#[test]
fn empty_string_is_empty_file() {
    let file = parse("").expect("empty source parses");
    assert!(file.items.is_empty());
}

/// Line numbers are preserved: a syntax error inside a block that starts partway
/// down the document reports the error at its REAL document line, not a line
/// relative to the extracted code.
#[test]
fn error_positions_map_to_original_document_lines() {
    // The `)` is malformed-missing on line 6 of the DOCUMENT.
    let doc = "\
# Title

prose line 3

```lamina
fn bad( { return 0; }
```
";
    let err = parse(doc).expect_err("malformed function must fail to parse");
    let msg = err.to_string();
    // Diagnostics carry a 1-based line; it must reference line 6 (the fn line),
    // proving the extractor preserved original line numbers rather than
    // reporting line 1 of the extracted code.
    assert!(
        msg.contains("line 6"),
        "expected an error on original document line 6, got: {msg}"
    );
}
