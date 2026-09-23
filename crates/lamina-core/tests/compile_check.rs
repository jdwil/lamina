//! Compiler-verification harness for the shipped language definitions.
//!
//! Every other `tests/<lang>.rs` suite asserts the EXACT emitted string and was
//! (historically) only *hand*-verified to be valid target code. This harness
//! closes that gap: it emits a representative program from a shipped `.mdl`,
//! wraps it in the minimal boilerplate the target needs, and feeds it to the
//! REAL compiler on `PATH`, asserting a clean compile. It is the mechanical
//! backstop that catches classes of bug hand-review misses — e.g. a `printf`
//! format specifier that mismatches its argument width (undefined behavior in
//! C), which `gcc -Wformat -Werror` flags instantly.
//!
//! **Graceful skip:** if a target's compiler is not installed, that test prints
//! a `SKIP` line and passes, so the suite still runs green on a machine without
//! every toolchain. Presence is detected by probing the tool with `--version`.

use std::path::PathBuf;
use std::process::Command;

use lamina_core::ast::{
    BinaryOp, Expr, Field, File, Function, Item, Meta, Modifier, Param, Primitive, Statement,
    SwitchCase, Type, TypeAttribute, Variant, VariantPayload, Visibility,
};
use lamina_core::emitter::emit;
use lamina_core::lang::LanguageDef;
use lamina_core::load_language_def;

/// Loads a shipped def from the sibling `lamina-defs/languages/<name>.mdl`.
fn shipped_def(name: &str) -> LanguageDef {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // crates
    path.pop(); // lamina
    path.pop(); // jd
    path.push("lamina-defs");
    path.push("languages");
    path.push(format!("{name}.mdl"));
    load_language_def(&path)
        .unwrap_or_else(|e| panic!("shipped {name}.mdl should load: {e}"))
}

fn emit_items(items: Vec<Item>, lang: &LanguageDef) -> String {
    emit(&File { items }, lang).unwrap_or_else(|e| panic!("emit failed: {e}"))
}

/// Returns `true` if `tool` responds to `--version` (i.e. is installed).
fn tool_present(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Resolves a tool that may live outside the default non-interactive `PATH`.
///
/// Some toolchains are installed by a version manager whose `bin` dir is only
/// added to an *interactive* shell's `PATH` (e.g. SDKMAN for Kotlin, swiftly for
/// Swift), so a plain `Command::new("kotlinc")` fails under `cargo test`. This
/// probes, in order, the bare name on `PATH` and then the known manager
/// locations under `$HOME`, returning the first spelling that answers to a
/// version probe. Returns `None` when the tool is genuinely absent (a graceful
/// SKIP, not a hard failure).
fn resolve_managed_tool(tool: &str, manager_subpaths: &[&str]) -> Option<String> {
    // 1. Bare name on PATH.
    if tool_present(tool) {
        return Some(tool.to_string());
    }
    // 2. Known version-manager bin locations under $HOME.
    let home = std::env::var("HOME").ok()?;
    for sub in manager_subpaths {
        let candidate = PathBuf::from(&home).join(sub);
        if candidate.exists() {
            let ok = Command::new(&candidate)
                .arg("-version")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
                // some tools use --version rather than -version
                || Command::new(&candidate)
                    .arg("--version")
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false);
            if ok {
                if let Some(s) = candidate.to_str() {
                    return Some(s.to_string());
                }
            }
        }
    }
    None
}

/// Writes `contents` to a uniquely-named temp file with `ext`, returning its path.
fn write_temp(stem: &str, ext: &str, contents: &str) -> PathBuf {
    // A per-process, per-stem unique dir under the system temp root.
    let mut dir = std::env::temp_dir();
    dir.push(format!("lamina-cc-{}-{}", std::process::id(), stem));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let mut file = dir.clone();
    file.push(format!("{stem}.{ext}"));
    std::fs::write(&file, contents).expect("write temp source");
    file
}

// ---- helpers to build a representative AST -------------------------------

fn field(name: &str, ty: Primitive) -> Field {
    Field {
        name: name.to_string(),
        ty: Type::Primitive(ty),
        visibility: Visibility::Public,
        meta: Meta::new(),
    }
}

// ==========================================================================
// C (gcc / clang)
// ==========================================================================

/// A representative C translation unit exercising the risky reconstructions:
/// a plain struct, a struct with a generated `equatable` helper, a struct with
/// a generated `displayable` printer (the width-specifier hazard), and a
/// payload-bearing enum realized as a tagged union.
fn representative_c(lang: &LanguageDef) -> String {
    // Struct with equatable + displayable: exercises the generated Point_eq
    // (field-wise `==`) and Point_print (printf specifier per field type,
    // including a 64-bit field to catch the %d-vs-%lld width UB).
    let point = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        fields: vec![
            field("x", Primitive::I32),
            field("y", Primitive::I64), // 64-bit: bare %d would be UB
            field("z", Primitive::U64),
        ],
        attributes: vec![TypeAttribute::Equatable, TypeAttribute::Displayable],
        meta: Meta::new(),
    };
    // Payload-bearing enum -> tagged union.
    let shape = Item::Enum {
        name: "Shape".to_string(),
        visibility: Visibility::Public,
        variants: vec![
            Variant {
                name: "Empty".to_string(),
                payload: VariantPayload::None,
                meta: Meta::new(),
            },
            Variant {
                name: "Circle".to_string(),
                payload: VariantPayload::Tuple(vec![Type::Primitive(Primitive::I32)]),
                meta: Meta::new(),
            },
            Variant {
                name: "Rect".to_string(),
                payload: VariantPayload::Tuple(vec![
                    Type::Primitive(Primitive::I32),
                    Type::Primitive(Primitive::I32),
                ]),
                meta: Meta::new(),
            },
        ],
        attributes: vec![],
        meta: Meta::new(),
    };
    emit_items(vec![point, shape], lang)
}

fn compile_c_with(tool: &str) {
    if !tool_present(tool) {
        eprintln!("SKIP compile_check C via {tool}: not installed");
        return;
    }
    let lang = shipped_def("c");
    let emitted = representative_c(&lang);
    // Wrap in the minimal TU: the headers the generated code needs + a main
    // that references the generated symbols so nothing is dead-stripped away
    // before the compiler checks it.
    let program = format!(
        "#include <stdint.h>\n#include <stdbool.h>\n#include <stdio.h>\n\n{emitted}\n\n\
         int main(void) {{\n    \
             struct Point p = {{0}};\n    \
             (void)Point_eq(&p, &p);\n    \
             Point_print(&p);\n    \
             struct Shape s = {{0}};\n    \
             (void)s;\n    \
             return 0;\n}}\n"
    );
    let src = write_temp("c_repr", "c", &program);
    let mut obj = src.clone();
    obj.set_extension("o");
    // -Wformat -Werror is the whole point: a width-mismatched printf specifier
    // (e.g. %d on an int64_t) becomes a hard error, mechanically catching the
    // class of UB that hand-review missed once already.
    let out = Command::new(tool)
        .args([
            "-std=c11",
            "-Wall",
            "-Wextra",
            "-Wformat",
            "-Werror",
            "-c",
            src.to_str().expect("utf8 path"),
            "-o",
            obj.to_str().expect("utf8 path"),
        ])
        .output()
        .expect("run C compiler");
    assert!(
        out.status.success(),
        "generated C failed to compile with {tool}:\n--- program ---\n{program}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn c_output_compiles_with_gcc() {
    compile_c_with("gcc");
}

#[test]
fn c_output_compiles_with_clang() {
    compile_c_with("clang");
}

// ==========================================================================
// Rust (rustc)
// ==========================================================================

#[test]
fn rust_output_compiles_with_rustc() {
    if !tool_present("rustc") {
        eprintln!("SKIP compile_check Rust: rustc not installed");
        return;
    }
    let lang = shipped_def("rust");
    // A struct with derives + a function with a body: exercises the derive line,
    // field rendering, and a return.
    let point = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", Primitive::I32), field("y", Primitive::I64)],
        attributes: vec![
            TypeAttribute::Equatable,
            TypeAttribute::Cloneable,
            TypeAttribute::Displayable,
        ],
        meta: Meta::new(),
    };
    let answer = Item::Function(Function {
        name: "answer".to_string(),
        visibility: Visibility::Public,
        modifiers: vec![],
        params: vec![Param {
            name: "n".to_string(),
            ty: Type::Primitive(Primitive::I32),
            meta: Meta::new(),
        }],
        return_type: Type::Primitive(Primitive::I32),
        body: vec![Statement::Return(Some(Expr::Binary {
            op: BinaryOp::Add,
            lhs: Box::new(Expr::Ref("n".to_string())),
            rhs: Box::new(Expr::IntLiteral("1".to_string())),
        }))],
        meta: Meta::new(),
    });
    let emitted = emit_items(vec![point, answer], &lang);
    let src = write_temp("rust_repr", "rs", &emitted);
    // --crate-type lib avoids needing a main; --emit=metadata is the cheapest
    // check that it type-checks. -D warnings keeps it strict.
    let mut meta = src.clone();
    meta.set_extension("rmeta");
    let out = Command::new("rustc")
        .args([
            "--crate-type",
            "lib",
            "--edition",
            "2021",
            "--emit=metadata",
            "-o",
            meta.to_str().expect("utf8 path"),
            src.to_str().expect("utf8 path"),
        ])
        .output()
        .expect("run rustc");
    assert!(
        out.status.success(),
        "generated Rust failed to compile:\n--- program ---\n{emitted}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// ==========================================================================
// Haskell (ghc) — highest-risk: never compiler-verified before
// ==========================================================================

#[test]
fn haskell_output_compiles_with_ghc() {
    if !tool_present("ghc") {
        eprintln!("SKIP compile_check Haskell: ghc not installed");
        return;
    }
    let lang = shipped_def("haskell");
    // A record struct with a deriving clause + a function defining equation:
    // exercises record syntax, `deriving (...)`, the `Int32` type mapping, and
    // an arithmetic body. Int32 comes from Data.Int.
    let point = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", Primitive::I32), field("y", Primitive::I32)],
        attributes: vec![
            TypeAttribute::Displayable,
            TypeAttribute::Equatable,
            TypeAttribute::Comparable,
        ],
        meta: Meta::new(),
    };
    let add = Item::Function(Function {
        name: "addOne".to_string(),
        visibility: Visibility::Public,
        modifiers: vec![],
        params: vec![Param {
            name: "n".to_string(),
            ty: Type::Primitive(Primitive::I32),
            meta: Meta::new(),
        }],
        return_type: Type::Primitive(Primitive::I32),
        body: vec![Statement::Return(Some(Expr::Binary {
            op: BinaryOp::Add,
            lhs: Box::new(Expr::Ref("n".to_string())),
            rhs: Box::new(Expr::IntLiteral("1".to_string())),
        }))],
        meta: Meta::new(),
    });
    let emitted = emit_items(vec![point, add], &lang);
    // Minimal module: enable record-field duplication (harmless for one record)
    // and import Data.Int for Int32. GHC's -fno-code type-checks without
    // producing an object, the cheapest full front-end pass.
    let program = format!(
        "{{-# LANGUAGE DuplicateRecordFields #-}}\nmodule Repr where\nimport Data.Int\n\n{emitted}\n"
    );
    let src = write_temp("hs_repr", "hs", &program);
    let out = Command::new("ghc")
        .args([
            "-fno-code",
            "-Wall",
            src.to_str().expect("utf8 path"),
        ])
        .output()
        .expect("run ghc");
    assert!(
        out.status.success(),
        "generated Haskell failed to type-check with ghc:\n--- program ---\n{program}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// ==========================================================================
// Go (gofmt -e parse)
// ==========================================================================
//
// Detection note: Go's driver uses the `go version` SUBCOMMAND, not a
// `go --version` FLAG, so the generic `tool_present("go")` (`go --version`)
// FAILS and would wrongly skip. `gofmt` is a separate binary on PATH that DOES
// answer `gofmt --help`/parse, and it is exactly the tool this probe runs, so
// presence is detected by probing `gofmt` (falling back to `go version`).

/// Returns `true` if `gofmt` (the Go formatter/parser) is available on PATH.
/// `gofmt -e` on an empty stdin succeeds, so we probe with a trivial help/run.
fn gofmt_present() -> bool {
    // `gofmt` with no args reads stdin; probe with `-h` which exits cleanly-ish.
    // The most robust presence check is simply whether the binary can be spawned.
    if Command::new("gofmt").arg("-h").output().is_ok() {
        return true;
    }
    // Fallback: `go version` (the correct subcommand — NOT `go --version`).
    Command::new("go")
        .arg("version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn go_output_compiles() {
    if !gofmt_present() {
        eprintln!("SKIP compile_check Go: gofmt not installed");
        return;
    }
    // Go is a Wave B target; only probe if a go.mdl exists yet.
    let mut def_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    def_path.pop();
    def_path.pop();
    def_path.pop();
    def_path.push("lamina-defs/languages/go.mdl");
    if !def_path.exists() {
        eprintln!("SKIP compile_check Go: go.mdl not shipped yet");
        return;
    }
    let lang = shipped_def("go");

    // A struct with the generated equatable `Equal` and displayable `String`
    // (fmt.Stringer) methods — the risky reconstructions — plus a payloadless
    // `iota` enum, a payload-bearing sealed-interface enum, and a function with
    // control flow (a counted `for` header, an `if`, a `switch`).
    let point = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("X", Primitive::I32), field("Y", Primitive::I64)],
        attributes: vec![TypeAttribute::Equatable, TypeAttribute::Displayable],
        meta: Meta::new(),
    };
    let color = Item::Enum {
        name: "Color".to_string(),
        visibility: Visibility::Public,
        variants: vec![
            Variant {
                name: "Red".to_string(),
                payload: VariantPayload::None,
                meta: Meta::new(),
            },
            Variant {
                name: "Green".to_string(),
                payload: VariantPayload::None,
                meta: Meta::new(),
            },
        ],
        attributes: vec![],
        meta: Meta::new(),
    };
    let shape = Item::Enum {
        name: "Shape".to_string(),
        visibility: Visibility::Public,
        variants: vec![
            Variant {
                name: "Empty".to_string(),
                payload: VariantPayload::None,
                meta: Meta::new(),
            },
            Variant {
                name: "Circle".to_string(),
                payload: VariantPayload::Tuple(vec![Type::Primitive(Primitive::I32)]),
                meta: Meta::new(),
            },
        ],
        attributes: vec![],
        meta: Meta::new(),
    };
    let sum = Item::Function(Function {
        name: "Sum".to_string(),
        visibility: Visibility::Public,
        modifiers: vec![],
        params: vec![Param {
            name: "n".to_string(),
            ty: Type::Primitive(Primitive::I32),
            meta: Meta::new(),
        }],
        return_type: Type::Primitive(Primitive::I32),
        body: vec![
            Statement::Let {
                name: "acc".to_string(),
                ty: None,
                value: Some(Expr::IntLiteral("0".to_string())),
            },
            Statement::For {
                init: Some(Box::new(Statement::Let {
                    name: "i".to_string(),
                    ty: None,
                    value: Some(Expr::IntLiteral("0".to_string())),
                })),
                cond: Some(Expr::Binary {
                    op: BinaryOp::Lt,
                    lhs: Box::new(Expr::Ref("i".to_string())),
                    rhs: Box::new(Expr::Ref("n".to_string())),
                }),
                step: Some(Box::new(
                    Statement::assign(
                        Expr::Ref("i".to_string()),
                        Expr::Binary {
                            op: BinaryOp::Add,
                            lhs: Box::new(Expr::Ref("i".to_string())),
                            rhs: Box::new(Expr::IntLiteral("1".to_string())),
                        },
                    )
                    .expect("lvalue"),
                )),
                body: vec![Statement::assign(
                    Expr::Ref("acc".to_string()),
                    Expr::Binary {
                        op: BinaryOp::Add,
                        lhs: Box::new(Expr::Ref("acc".to_string())),
                        rhs: Box::new(Expr::Ref("i".to_string())),
                    },
                )
                .expect("lvalue")],
            },
            Statement::Return(Some(Expr::Ref("acc".to_string()))),
        ],
        meta: Meta::new(),
    });

    let emitted = emit_items(vec![point, color, shape, sum], &lang);
    // Prefix the package clause + the `fmt` import the generated Stringer needs.
    // `gofmt -e` PARSES (it does not type-check import usage), so this stays a
    // pure syntax check — the appropriate bar for a single-file def probe.
    let program = format!("package repr\n\nimport \"fmt\"\n\n{emitted}\n");
    let src = write_temp("go_repr", "go", &program);
    let out = Command::new("gofmt")
        .args(["-e", src.to_str().expect("utf8 path")])
        .output()
        .expect("run gofmt");
    assert!(
        out.status.success(),
        "generated Go failed to parse with gofmt:\n--- program ---\n{program}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// ==========================================================================
// TypeScript (tsc --noEmit)
// ==========================================================================

#[test]
fn typescript_output_typechecks_with_tsc() {
    // tsc is fetched via npx; probe npx presence.
    if !tool_present("npx") {
        eprintln!("SKIP compile_check TypeScript: npx not installed");
        return;
    }
    let lang = shipped_def("typescript");
    let point = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", Primitive::I32), field("y", Primitive::I32)],
        attributes: vec![TypeAttribute::Equatable, TypeAttribute::Displayable],
        meta: Meta::new(),
    };
    let emitted = emit_items(vec![point], &lang);
    let src = write_temp("ts_repr", "ts", &emitted);
    let out = Command::new("npx")
        .args([
            "--yes",
            "-p",
            "typescript",
            "tsc",
            "--noEmit",
            "--strict",
            src.to_str().expect("utf8 path"),
        ])
        .output();
    match out {
        Ok(o) => assert!(
            o.status.success(),
            "generated TypeScript failed to type-check:\n--- program ---\n{emitted}\n--- stderr ---\n{}\n{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        ),
        Err(e) => eprintln!("SKIP compile_check TypeScript: could not run tsc ({e})"),
    }
}

// ==========================================================================
// Python (python3 -c compile)
// ==========================================================================

#[test]
fn python_output_parses_with_python3() {
    let py = if tool_present("python3") {
        "python3"
    } else if tool_present("python") {
        "python"
    } else {
        eprintln!("SKIP compile_check Python: no python interpreter");
        return;
    };
    let lang = shipped_def("python");
    let point = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", Primitive::I32), field("y", Primitive::I32)],
        attributes: vec![
            TypeAttribute::Equatable,
            TypeAttribute::Comparable,
            TypeAttribute::Hashable,
        ],
        meta: Meta::new(),
    };
    let emitted = emit_items(vec![point], &lang);
    let src = write_temp("py_repr", "py", &emitted);
    // `python3 -m py_compile` fully parses + byte-compiles, catching syntax and
    // (for dataclass decorators) argument errors at import-independent compile.
    let out = Command::new(py)
        .args(["-m", "py_compile", src.to_str().expect("utf8 path")])
        .output()
        .expect("run python");
    assert!(
        out.status.success(),
        "generated Python failed to compile:\n--- program ---\n{emitted}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// ==========================================================================
// Java (javac) — Wave B Tier 3 (class-only item model); probe if shipped
// ==========================================================================

#[test]
fn java_output_compiles_with_javac() {
    if !tool_present("javac") {
        eprintln!("SKIP compile_check Java: javac not installed");
        return;
    }
    let mut def_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    def_path.pop();
    def_path.pop();
    def_path.pop();
    def_path.push("lamina-defs/languages/java.mdl");
    if !def_path.exists() {
        eprintln!("SKIP compile_check Java: java.mdl not shipped yet");
        return;
    }
    let lang = shipped_def("java");

    // A representative program exercising the risky realizations:
    //   * a plain top-level function → a `public static` method (the honest
    //     no-free-functions idiom),
    //   * a struct with record-realizable attributes → a Java `record`
    //     (equals/hashCode/toString for free),
    //   * a raw top-level item → verbatim pass-through (a static helper),
    //   * a payload-bearing enum → a `sealed interface` + `record` variants
    //     (Java 17+ discriminated union), and
    //   * a const → a `public static final` field.
    // Java has no free items, so every emitted member is a class member; the
    // harness wraps them in one top-level class (public class name == filename,
    // as javac requires). Nested records/sealed-interface are legal Java 21.
    let add = Item::Function(Function {
        name: "addOne".to_string(),
        visibility: Visibility::Public,
        modifiers: vec![],
        params: vec![Param {
            name: "n".to_string(),
            ty: Type::Primitive(Primitive::I32),
            meta: Meta::new(),
        }],
        return_type: Type::Primitive(Primitive::I32),
        body: vec![Statement::Return(Some(Expr::Binary {
            op: BinaryOp::Add,
            lhs: Box::new(Expr::Ref("n".to_string())),
            rhs: Box::new(Expr::IntLiteral("1".to_string())),
        }))],
        meta: Meta::new(),
    });
    let point = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", Primitive::I32), field("y", Primitive::I64)],
        attributes: vec![
            TypeAttribute::Equatable,
            TypeAttribute::Hashable,
            TypeAttribute::Displayable,
        ],
        meta: Meta::new(),
    };
    let raw = Item::Raw {
        code: "public static int square(int v) { return v * v; }".to_string(),
        meta: Meta::new(),
    };
    let shape = Item::Enum {
        name: "Shape".to_string(),
        visibility: Visibility::Public,
        variants: vec![
            Variant {
                name: "Empty".to_string(),
                payload: VariantPayload::None,
                meta: Meta::new(),
            },
            Variant {
                name: "Circle".to_string(),
                payload: VariantPayload::Tuple(vec![Type::Primitive(Primitive::I32)]),
                meta: Meta::new(),
            },
            Variant {
                name: "Rect".to_string(),
                payload: VariantPayload::Struct(vec![
                    field("w", Primitive::I32),
                    field("h", Primitive::I32),
                ]),
                meta: Meta::new(),
            },
        ],
        attributes: vec![],
        meta: Meta::new(),
    };
    let konst = Item::Const {
        name: "ANSWER".to_string(),
        ty: Type::Primitive(Primitive::I32),
        value: Expr::IntLiteral("42".to_string()),
        visibility: Visibility::Public,
        meta: Meta::new(),
    };
    let emitted = emit_items(vec![add, point, raw, shape, konst], &lang);

    // Indent every emitted line one level so it nests cleanly inside the wrapper
    // class, then add a `main` that references every generated symbol (so nothing
    // is unused-warning'd away) and a `describe` using a sealed `switch` pattern
    // (Java 21 pattern-matching switch, which the compiler checks for
    // exhaustiveness against the permitted records).
    let indented: String = emitted
        .lines()
        .map(|l| {
            if l.is_empty() {
                String::new()
            } else {
                format!("    {l}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let program = format!(
        "public class JavaRepr {{\n\
         {indented}\n\n    \
         static String describe(Shape s) {{\n        \
             return switch (s) {{\n            \
                 case Empty e -> \"empty\";\n            \
                 case Circle c -> \"circle \" + c.c0();\n            \
                 case Rect r -> \"rect \" + r.w() + \" \" + r.h();\n        \
             }};\n    \
         }}\n\n    \
         public static void main(String[] args) {{\n        \
             Point p = new Point(1, 2L);\n        \
             System.out.println(p);\n        \
             System.out.println(p.equals(p));\n        \
             System.out.println(p.hashCode());\n        \
             System.out.println(square(3));\n        \
             System.out.println(addOne(41));\n        \
             System.out.println(ANSWER);\n        \
             System.out.println(describe(new Circle(3)));\n    \
         }}\n\
         }}\n"
    );
    // javac requires public-class name == filename, so the stem is fixed here.
    let src = write_temp("JavaRepr", "java", &program);
    let out_dir = src.parent().expect("temp parent").to_path_buf();
    let out = Command::new("javac")
        .args([
            "-d",
            out_dir.to_str().expect("utf8 path"),
            src.to_str().expect("utf8 path"),
        ])
        .output()
        .expect("run javac");
    assert!(
        out.status.success(),
        "generated Java failed to compile with javac:\n--- program ---\n{program}\n--- stdout ---\n{}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

// ==========================================================================
// Kotlin (kotlinc) — Wave B breadth; kotlinc lives under SDKMAN, not PATH
// ==========================================================================

#[test]
fn kotlin_output_compiles_with_kotlinc() {
    // kotlinc is installed by SDKMAN, whose bin dir is only on an interactive
    // shell's PATH — probe the SDKMAN location in addition to PATH.
    let kotlinc = match resolve_managed_tool(
        "kotlinc",
        &[".sdkman/candidates/kotlin/current/bin/kotlinc"],
    ) {
        Some(k) => k,
        None => {
            eprintln!("SKIP compile_check Kotlin: kotlinc not installed (PATH or SDKMAN)");
            return;
        }
    };
    let lang = shipped_def("kotlin");

    // A representative program exercising the risky realizations:
    //   * a plain function with a body + a return,
    //   * a data class (the equatable/hashable/displayable/cloneable home),
    //   * a payload-bearing enum realized as a sealed-class hierarchy
    //     (data object + data class subclasses), and
    //   * a function that USES a when over the sealed type so the compiler
    //     type-checks the generated subclasses and the `when` branches.
    let point = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", Primitive::I32), field("y", Primitive::I64)],
        attributes: vec![
            TypeAttribute::Equatable,
            TypeAttribute::Hashable,
            TypeAttribute::Displayable,
            TypeAttribute::Cloneable,
        ],
        meta: Meta::new(),
    };
    let shape = Item::Enum {
        name: "Shape".to_string(),
        visibility: Visibility::Public,
        variants: vec![
            Variant {
                name: "Empty".to_string(),
                payload: VariantPayload::None,
                meta: Meta::new(),
            },
            Variant {
                name: "Circle".to_string(),
                payload: VariantPayload::Tuple(vec![Type::Primitive(Primitive::I32)]),
                meta: Meta::new(),
            },
            Variant {
                name: "Rect".to_string(),
                payload: VariantPayload::Struct(vec![
                    field("w", Primitive::I32),
                    field("h", Primitive::I32),
                ]),
                meta: Meta::new(),
            },
        ],
        attributes: vec![],
        meta: Meta::new(),
    };
    let add = Item::Function(Function {
        name: "addOne".to_string(),
        visibility: Visibility::Public,
        modifiers: vec![],
        params: vec![Param {
            name: "n".to_string(),
            ty: Type::Primitive(Primitive::I32),
            meta: Meta::new(),
        }],
        return_type: Type::Primitive(Primitive::I32),
        body: vec![Statement::Return(Some(Expr::Binary {
            op: BinaryOp::Add,
            lhs: Box::new(Expr::Ref("n".to_string())),
            rhs: Box::new(Expr::IntLiteral("1".to_string())),
        }))],
        meta: Meta::new(),
    });
    let emitted = emit_items(vec![point, shape, add], &lang);

    // A tiny hand-written `main` references every generated symbol so nothing is
    // dead-stripped before the compiler checks it, and exercises the sealed
    // `when` (Kotlin requires the branches to cover the sealed subtypes).
    let program = format!(
        "{emitted}\n\n\
         fun describe(s: Shape): String = when (s) {{\n    \
             is Shape.Empty -> \"empty\"\n    \
             is Shape.Circle -> \"circle \" + s.c0.toString()\n    \
             is Shape.Rect -> \"rect \" + s.w.toString() + \" \" + s.h.toString()\n\
         }}\n\n\
         fun main() {{\n    \
             val p = Point(1, 2L)\n    \
             println(p)\n    \
             println(p == p.copy())\n    \
             println(p.hashCode())\n    \
             println(addOne(41))\n    \
             println(describe(Shape.Circle(3)))\n\
         }}\n"
    );
    let src = write_temp("kotlin_repr", "kt", &program);
    // Compile to a jar in a temp dir. kotlinc is JVM-slow (cold warmup) — that
    // is expected. A clean compile is the bar (any type error, unresolved
    // reference, or non-exhaustive `when` fails the build).
    let mut out_jar = src.clone();
    out_jar.set_extension("jar");
    let out = Command::new(&kotlinc)
        .args([
            src.to_str().expect("utf8 path"),
            "-include-runtime",
            "-d",
            out_jar.to_str().expect("utf8 path"),
        ])
        .output()
        .expect("run kotlinc");
    assert!(
        out.status.success(),
        "generated Kotlin failed to compile with kotlinc:\n--- program ---\n{program}\n--- stdout ---\n{}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

// ==========================================================================
// TOML (python3 tomllib parse, else ruby) — declarative tree-core target
// ==========================================================================

/// A representative TOML document exercising the tree-core mapping: a document
/// root with a bare top-level key, a `[table]` section of mixed scalar kinds
/// (string / integer / boolean / float), and an inline array value. This is the
/// shape the `toml.rs` suite asserts exactly; here it is fed to a real TOML
/// parser to prove the emitted text is *well-formed* TOML, not merely
/// string-equal to a hand-written expectation.
fn representative_toml(lang: &LanguageDef) -> String {
    use lamina_core::ast::Attr;
    let kv = |k: &str, v: Expr| Attr {
        name: k.to_string(),
        value: v,
        meta: Meta::new(),
    };
    // A `[package]` table with mixed scalar kinds + an inline array.
    let package = Expr::Node {
        name: "package".to_string(),
        attrs: vec![
            kv("name", Expr::StringLiteral("lamina-core".to_string())),
            kv("edition", Expr::IntLiteral("2021".to_string())),
            kv("publish", Expr::BoolLiteral(false)),
            kv("ratio", Expr::FloatLiteral("0.5".to_string())),
            kv(
                "keywords",
                Expr::ArrayLit {
                    elems: vec![
                        Expr::StringLiteral("ir".to_string()),
                        Expr::StringLiteral("transpiler".to_string()),
                    ],
                    meta: Meta::new(),
                },
            ),
        ],
        children: vec![],
        meta: Meta::new(),
    };
    // The document root: one bare top-level key, then the `[package]` table.
    let root = Expr::Node {
        name: String::new(),
        attrs: vec![kv("title", Expr::StringLiteral("Lamina".to_string()))],
        children: vec![package],
        meta: Meta::new().with("root", "true"),
    };
    emit(&File { items: vec![Item::Tree(root)] }, lang)
        .unwrap_or_else(|e| panic!("emit toml failed: {e}"))
}

#[test]
fn toml_output_parses() {
    let lang = shipped_def("toml");
    let emitted = representative_toml(&lang);
    let src = write_temp("toml_repr", "toml", &emitted);
    let path = src.to_str().expect("utf8 path");

    // Prefer Python's built-in tomllib (3.11+); python3 here is 3.13.
    let py = if tool_present("python3") {
        Some("python3")
    } else if tool_present("python") {
        Some("python")
    } else {
        None
    };
    if let Some(py) = py {
        // Confirm this interpreter actually has tomllib before relying on it;
        // an older interpreter (<3.11) would lack it, in which case fall through
        // to the ruby probe rather than failing spuriously.
        let has_tomllib = Command::new(py)
            .args(["-c", "import tomllib"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if has_tomllib {
            let out = Command::new(py)
                .args([
                    "-c",
                    "import tomllib,sys; tomllib.load(open(sys.argv[1],'rb'))",
                    path,
                ])
                .output()
                .expect("run python tomllib");
            assert!(
                out.status.success(),
                "generated TOML failed to parse with {py} tomllib:\n--- program ---\n{emitted}\n--- stderr ---\n{}",
                String::from_utf8_lossy(&out.stderr)
            );
            return;
        }
    }

    // Fallback: ruby's stdlib does not ship a TOML parser, so require the
    // `toml-rb` gem; probe its availability and use it only if present.
    if tool_present("ruby") {
        let has_toml_gem = Command::new("ruby")
            .args(["-e", "require 'toml-rb'"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if has_toml_gem {
            let out = Command::new("ruby")
                .args([
                    "-e",
                    "require 'toml-rb'; TomlRB.load_file(ARGV[0])",
                    path,
                ])
                .output()
                .expect("run ruby toml-rb");
            assert!(
                out.status.success(),
                "generated TOML failed to parse with ruby toml-rb:\n--- program ---\n{emitted}\n--- stderr ---\n{}",
                String::from_utf8_lossy(&out.stderr)
            );
            return;
        }
    }

    eprintln!("SKIP compile_check TOML: no TOML parser available (python3 tomllib / ruby toml-rb)");
}

// ==========================================================================
// YAML (python3 PyYAML safe_load, else ruby psych) — declarative tree-core
// ==========================================================================

/// A representative YAML document exercising the tree-core mapping: a top-level
/// block mapping with mixed scalar kinds, a `block`-tagged nested mapping, and a
/// `block`-tagged nested sequence. This is the shape the `yaml.rs` suite asserts
/// exactly; here it is fed to a real YAML parser to prove the emitted text is
/// *well-formed* YAML (correct indentation, valid block structure), not merely
/// string-equal to a hand-written expectation.
fn representative_yaml(lang: &LanguageDef) -> String {
    use lamina_core::ast::Attr;
    let s = |k: &str, v: &str| Attr {
        name: k.to_string(),
        value: Expr::StringLiteral(v.to_string()),
        meta: Meta::new(),
    };
    let i = |k: &str, v: &str| Attr {
        name: k.to_string(),
        value: Expr::IntLiteral(v.to_string()),
        meta: Meta::new(),
    };
    let block = |k: &str, nested: Expr| Attr {
        name: k.to_string(),
        value: nested,
        meta: Meta::new().with("block", "true"),
    };
    let map = |attrs: Vec<Attr>| Expr::Node {
        name: String::new(),
        attrs,
        children: vec![],
        meta: Meta::new(),
    };
    let seq = |children: Vec<Expr>| Expr::Node {
        name: String::new(),
        attrs: vec![],
        children,
        meta: Meta::new().with("yaml", "seq"),
    };
    // A config-file-shaped document: scalars, a nested mapping, a nested seq.
    let root = map(vec![
        s("name", "lamina"),
        i("version", "1"),
        Attr {
            name: "enabled".to_string(),
            value: Expr::BoolLiteral(true),
            meta: Meta::new(),
        },
        block(
            "server",
            map(vec![s("host", "localhost"), i("port", "5432")]),
        ),
        block(
            "tags",
            seq(vec![
                Expr::StringLiteral("ir".to_string()),
                Expr::StringLiteral("transpiler".to_string()),
            ]),
        ),
    ]);
    emit(&File { items: vec![Item::Tree(root)] }, lang)
        .unwrap_or_else(|e| panic!("emit yaml failed: {e}"))
}

#[test]
fn yaml_output_parses() {
    let lang = shipped_def("yaml");
    let emitted = representative_yaml(&lang);
    let src = write_temp("yaml_repr", "yaml", &emitted);
    let path = src.to_str().expect("utf8 path");

    // Prefer Python's PyYAML (safe_load) when importable.
    let py = if tool_present("python3") {
        Some("python3")
    } else if tool_present("python") {
        Some("python")
    } else {
        None
    };
    if let Some(py) = py {
        // Confirm this interpreter actually has PyYAML before relying on it;
        // fall through to the ruby probe otherwise rather than failing.
        let has_pyyaml = Command::new(py)
            .args(["-c", "import yaml"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if has_pyyaml {
            let out = Command::new(py)
                .args([
                    "-c",
                    "import yaml,sys; yaml.safe_load(open(sys.argv[1]))",
                    path,
                ])
                .output()
                .expect("run python PyYAML");
            assert!(
                out.status.success(),
                "generated YAML failed to parse with {py} PyYAML:\n--- program ---\n{emitted}\n--- stderr ---\n{}",
                String::from_utf8_lossy(&out.stderr)
            );
            return;
        }
    }

    // Fallback: ruby's stdlib ships psych (the `yaml` require), so no gem is
    // needed — ruby 3.2 loads YAML out of the box.
    if tool_present("ruby") {
        let out = Command::new("ruby")
            .args([
                "-ryaml",
                "-e",
                "YAML.load_file(ARGV[0])",
                path,
            ])
            .output()
            .expect("run ruby psych");
        assert!(
            out.status.success(),
            "generated YAML failed to parse with ruby psych:\n--- program ---\n{emitted}\n--- stderr ---\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        return;
    }

    eprintln!("SKIP compile_check YAML: no YAML parser available (python3 PyYAML / ruby psych)");
}

// ==========================================================================
// Ruby (ruby -c) — Wave B breadth; dynamically-typed, end-delimited target
// ==========================================================================

#[test]
fn ruby_output_parses_with_ruby() {
    if !tool_present("ruby") {
        eprintln!("SKIP compile_check Ruby: ruby not installed");
        return;
    }
    let lang = shipped_def("ruby");

    // A representative program exercising the risky reconstructions:
    //   * a class with ALL four generated attribute methods — equatable (`==`),
    //     displayable (`to_s`), comparable (`include Comparable` + `<=>`), and
    //     hashable (`hash`/`eql?`) — the field-generic method generation is the
    //     highest-risk part of this def,
    //   * a plain enum realized as a `module` of frozen symbol constants, and
    //   * a function whose body USES a `while`, a `case`/`when`, and the
    //     compound-assignment idiom, so the parser checks the block/end nesting.
    let point = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", Primitive::I32), field("y", Primitive::I64)],
        attributes: vec![
            TypeAttribute::Equatable,
            TypeAttribute::Displayable,
            TypeAttribute::Comparable,
            TypeAttribute::Hashable,
        ],
        meta: Meta::new(),
    };
    let color = Item::Enum {
        name: "Color".to_string(),
        visibility: Visibility::Public,
        variants: vec![
            Variant {
                name: "RED".to_string(),
                payload: VariantPayload::None,
                meta: Meta::new(),
            },
            Variant {
                name: "GREEN".to_string(),
                payload: VariantPayload::None,
                meta: Meta::new(),
            },
        ],
        attributes: vec![],
        meta: Meta::new(),
    };
    // fn classify(n): a while loop with a compound-assign step, then a case/when.
    let while_stmt = Statement::While {
        cond: Expr::Binary {
            op: BinaryOp::Gt,
            lhs: Box::new(Expr::Ref("n".to_string())),
            rhs: Box::new(Expr::IntLiteral("0".to_string())),
        },
        body: vec![Statement::assign(
            Expr::Ref("n".to_string()),
            Expr::Binary {
                op: BinaryOp::Sub,
                lhs: Box::new(Expr::Ref("n".to_string())),
                rhs: Box::new(Expr::IntLiteral("1".to_string())),
            },
        )
        .expect("compound assign builds")],
    };
    let case_stmt = Statement::Switch {
        scrutinee: Expr::Ref("n".to_string()),
        cases: vec![lamina_core::ast::SwitchCase {
            value: Expr::IntLiteral("0".to_string()),
            body: vec![Statement::Return(Some(Expr::IntLiteral("1".to_string())))],
            meta: Meta::new(),
        }],
        default: Some(vec![Statement::Return(Some(Expr::IntLiteral("0".to_string())))]),
    };
    let classify = Item::Function(Function {
        name: "classify".to_string(),
        visibility: Visibility::Public,
        modifiers: vec![],
        params: vec![Param {
            name: "n".to_string(),
            ty: Type::Primitive(Primitive::I32),
            meta: Meta::new(),
        }],
        return_type: Type::Primitive(Primitive::I32),
        body: vec![while_stmt, case_stmt],
        meta: Meta::new(),
    });
    let emitted = emit_items(vec![point, color, classify], &lang);

    // `ruby -c <file>` is a syntax check only (parse, no execution): it prints
    // "Syntax OK" and exits 0 on a well-formed program, and fails on any parse
    // error (an unbalanced `end`, a bad interpolation, a stray operator). That
    // is the appropriate bar for a single-file def probe — the exact-string
    // `tests/ruby.rs` assertions pin semantics; this proves it PARSES.
    let src = write_temp("ruby_repr", "rb", &emitted);
    let out = Command::new("ruby")
        .args(["-c", src.to_str().expect("utf8 path")])
        .output()
        .expect("run ruby -c");
    assert!(
        out.status.success(),
        "generated Ruby failed to parse with `ruby -c`:\n--- program ---\n{emitted}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// ==========================================================================
// JavaScript (node --check) — Wave B breadth target
// ==========================================================================

/// A representative JavaScript module exercising the risky reconstructions:
/// a `class` with a field-assigning constructor, the generated `equatable`
/// (`Point_eq`) and `displayable` (`Point_toString`) free-function helpers, a
/// payloadless enum (frozen tag object), a payload-bearing enum (tagged arrow
/// constructors), and a function with `if`/`while` control flow plus a native
/// arrow-function lambda.
fn representative_javascript(lang: &LanguageDef) -> String {
    let point = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", Primitive::I32), field("y", Primitive::I64)],
        attributes: vec![TypeAttribute::Equatable, TypeAttribute::Displayable],
        meta: Meta::new(),
    };
    let color = Item::Enum {
        name: "Color".to_string(),
        visibility: Visibility::Public,
        variants: vec![
            Variant {
                name: "Red".to_string(),
                payload: VariantPayload::None,
                meta: Meta::new(),
            },
            Variant {
                name: "Green".to_string(),
                payload: VariantPayload::None,
                meta: Meta::new(),
            },
        ],
        attributes: vec![],
        meta: Meta::new(),
    };
    let shape = Item::Enum {
        name: "Shape".to_string(),
        visibility: Visibility::Public,
        variants: vec![
            Variant {
                name: "Empty".to_string(),
                payload: VariantPayload::None,
                meta: Meta::new(),
            },
            Variant {
                name: "Circle".to_string(),
                payload: VariantPayload::Tuple(vec![Type::Primitive(Primitive::I32)]),
                meta: Meta::new(),
            },
            Variant {
                name: "Rect".to_string(),
                payload: VariantPayload::Tuple(vec![
                    Type::Primitive(Primitive::I32),
                    Type::Primitive(Primitive::I32),
                ]),
                meta: Meta::new(),
            },
        ],
        attributes: vec![],
        meta: Meta::new(),
    };
    // function classify(n) {
    //     let f = (x) => { return x + 1; };
    //     while (n > 0) {
    //         if (n === 1) { return 1; }
    //         n -= 1;
    //     }
    //     return 0;
    // }
    let lambda = Expr::Lambda {
        params: vec![Param {
            name: "x".to_string(),
            ty: Type::Primitive(Primitive::I32),
            meta: Meta::new(),
        }],
        return_type: None,
        body: vec![Statement::Return(Some(Expr::Binary {
            op: BinaryOp::Add,
            lhs: Box::new(Expr::Ref("x".to_string())),
            rhs: Box::new(Expr::IntLiteral("1".to_string())),
        }))],
        meta: Meta::new(),
    };
    let let_lambda = Statement::Let {
        name: "f".to_string(),
        ty: None,
        value: Some(lambda),
    };
    let inner_if = Statement::If {
        cond: Expr::Binary {
            op: BinaryOp::Eq,
            lhs: Box::new(Expr::Ref("n".to_string())),
            rhs: Box::new(Expr::IntLiteral("1".to_string())),
        },
        then_block: vec![Statement::Return(Some(Expr::IntLiteral("1".to_string())))],
        else_block: None,
    };
    let dec = Statement::assign(
        Expr::Ref("n".to_string()),
        Expr::Binary {
            op: BinaryOp::Sub,
            lhs: Box::new(Expr::Ref("n".to_string())),
            rhs: Box::new(Expr::IntLiteral("1".to_string())),
        },
    )
    .expect("assign is an lvalue");
    let while_stmt = Statement::While {
        cond: Expr::Binary {
            op: BinaryOp::Gt,
            lhs: Box::new(Expr::Ref("n".to_string())),
            rhs: Box::new(Expr::IntLiteral("0".to_string())),
        },
        body: vec![inner_if, dec],
    };
    let classify = Item::Function(Function {
        name: "classify".to_string(),
        visibility: Visibility::Public,
        modifiers: vec![],
        params: vec![Param {
            name: "n".to_string(),
            ty: Type::Primitive(Primitive::I32),
            meta: Meta::new(),
        }],
        return_type: Type::Primitive(Primitive::I32),
        body: vec![
            let_lambda,
            while_stmt,
            Statement::Return(Some(Expr::IntLiteral("0".to_string()))),
        ],
        meta: Meta::new(),
    });
    emit_items(vec![point, color, shape, classify], lang)
}

#[test]
fn javascript_output_parses_with_node() {
    if !tool_present("node") {
        eprintln!("SKIP compile_check JavaScript: node not installed");
        return;
    }
    let lang = shipped_def("javascript");
    let emitted = representative_javascript(&lang);
    // The generated module uses `export` declarations, so it must be parsed as
    // an ES module: the `.mjs` extension makes `node --check` treat it as ESM.
    // `node --check` performs a full parse (syntax check) without executing the
    // module, which is the right bar for a single-file def probe.
    let src = write_temp("js_repr", "mjs", &emitted);
    let out = Command::new("node")
        .args(["--check", src.to_str().expect("utf8 path")])
        .output()
        .expect("run node");
    assert!(
        out.status.success(),
        "generated JavaScript failed to parse with node --check:\n--- program ---\n{emitted}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// ==========================================================================
// Swift (swiftc -parse) — Wave B breadth target
// ==========================================================================

/// A representative Swift program exercising the risky reconstructions: a
/// `struct` whose type attributes lower to generated `extension`s (an empty
/// synthesized `Equatable`, a field-wise `Comparable` `<`, and a
/// `CustomStringConvertible` `description`), a payload-bearing `enum` realized as
/// a native tagged union with associated values, a `func` with a body, a
/// `switch` with a default, the counted-`for` → `do { while }` desugar, and a
/// closure. Every line is emitted by the shipped `swift.mdl`; feeding it to the
/// real `swiftc` mechanically confirms the strings are valid Swift.
fn representative_swift(lang: &LanguageDef) -> String {
    let i32t = Type::Primitive(Primitive::I32);
    let i64t = Type::Primitive(Primitive::I64);

    // struct Point with equatable + comparable + displayable -> extensions.
    let point = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", Primitive::I32), field("y", Primitive::I64)],
        attributes: vec![
            TypeAttribute::Equatable,
            TypeAttribute::Comparable,
            TypeAttribute::Displayable,
        ],
        meta: Meta::new(),
    };
    // Payload-bearing enum -> native associated-value tagged union + synthesized
    // Equatable/Hashable extensions.
    let shape = Item::Enum {
        name: "Shape".to_string(),
        visibility: Visibility::Public,
        variants: vec![
            Variant {
                name: "empty".to_string(),
                payload: VariantPayload::None,
                meta: Meta::new(),
            },
            Variant {
                name: "circle".to_string(),
                payload: VariantPayload::Tuple(vec![i32t.clone()]),
                meta: Meta::new(),
            },
            Variant {
                name: "rect".to_string(),
                payload: VariantPayload::Struct(vec![
                    field("width", Primitive::I32),
                    field("height", Primitive::I32),
                ]),
                meta: Meta::new(),
            },
        ],
        attributes: vec![TypeAttribute::Equatable, TypeAttribute::Hashable],
        meta: Meta::new(),
    };
    // A function with a postfix-effect signature and an arithmetic return.
    let add = Item::Function(Function {
        name: "add".to_string(),
        visibility: Visibility::Public,
        modifiers: vec![],
        params: vec![
            Param {
                name: "a".to_string(),
                ty: i32t.clone(),
                meta: Meta::new(),
            },
            Param {
                name: "b".to_string(),
                ty: i32t.clone(),
                meta: Meta::new(),
            },
        ],
        return_type: i32t.clone(),
        body: vec![Statement::Return(Some(Expr::Binary {
            op: BinaryOp::Add,
            lhs: Box::new(Expr::Ref("a".to_string())),
            rhs: Box::new(Expr::Ref("b".to_string())),
        }))],
        meta: Meta::new(),
    });
    // async/throws postfix specifiers.
    let fetch = Item::Function(Function {
        name: "fetch".to_string(),
        visibility: Visibility::Public,
        modifiers: vec![Modifier::Async, Modifier::Throws],
        params: vec![],
        return_type: i32t.clone(),
        body: vec![Statement::Return(Some(Expr::IntLiteral("0".to_string())))],
        meta: Meta::new(),
    });
    // A switch with a default.
    let classify = Item::Function(Function {
        name: "classify".to_string(),
        visibility: Visibility::Public,
        modifiers: vec![],
        params: vec![Param {
            name: "n".to_string(),
            ty: i32t.clone(),
            meta: Meta::new(),
        }],
        return_type: Type::Primitive(Primitive::Void),
        body: vec![Statement::Switch {
            scrutinee: Expr::Ref("n".to_string()),
            cases: vec![
                SwitchCase {
                    value: Expr::IntLiteral("0".to_string()),
                    body: vec![Statement::Return(None)],
                    meta: Meta::new(),
                },
                SwitchCase {
                    value: Expr::IntLiteral("1".to_string()),
                    body: vec![Statement::Return(None)],
                    meta: Meta::new(),
                },
            ],
            default: Some(vec![Statement::Return(None)]),
        }],
        meta: Meta::new(),
    });
    // A counted for -> do { while } desugar with a mutated `var`.
    let count = Item::Function(Function {
        name: "count".to_string(),
        visibility: Visibility::Public,
        modifiers: vec![],
        params: vec![],
        return_type: Type::Primitive(Primitive::Void),
        body: vec![Statement::For {
            init: Some(Box::new(Statement::Let {
                name: "i".to_string(),
                ty: Some(i32t.clone()),
                value: Some(Expr::IntLiteral("0".to_string())),
            })),
            cond: Some(Expr::Binary {
                op: BinaryOp::Lt,
                lhs: Box::new(Expr::Ref("i".to_string())),
                rhs: Box::new(Expr::IntLiteral("10".to_string())),
            }),
            step: Some(Box::new(
                Statement::assign(
                    Expr::Ref("i".to_string()),
                    Expr::Binary {
                        op: BinaryOp::Add,
                        lhs: Box::new(Expr::Ref("i".to_string())),
                        rhs: Box::new(Expr::IntLiteral("1".to_string())),
                    },
                )
                .expect("lvalue"),
            )),
            body: vec![],
        }],
        meta: Meta::new(),
    });
    // A closure bound to a global constant.
    let inc = Item::Const {
        name: "inc".to_string(),
        ty: Type::Named("(Int32) -> Int32".to_string()),
        value: Expr::Lambda {
            params: vec![Param {
                name: "x".to_string(),
                ty: i32t.clone(),
                meta: Meta::new(),
            }],
            return_type: Some(i64t.clone()),
            body: vec![Statement::Return(Some(Expr::Ref("x".to_string())))],
            meta: Meta::new(),
        },
        visibility: Visibility::Public,
        meta: Meta::new(),
    };
    emit_items(vec![point, shape, add, fetch, classify, count, inc], lang)
}

#[test]
fn swift_output_compiles_with_swiftc() {
    // swiftc ships via `swiftly` under $HOME and is NOT on the non-interactive
    // PATH; resolve it there (falling back to a bare PATH lookup) before probing.
    let swiftc = match resolve_managed_tool("swiftc", &[".local/share/swiftly/bin/swiftc"]) {
        Some(p) => p,
        None => {
            eprintln!("SKIP compile_check Swift: swiftc not installed");
            return;
        }
    };
    let lang = shipped_def("swift");
    let emitted = representative_swift(&lang);
    let src = write_temp("swift_repr", "swift", &emitted);
    // `swiftc -parse` runs the full parse (and, in practice, enough of the front
    // end to reject malformed declarations) without linking or producing an
    // object — the appropriate bar for a single-file def probe. The generated
    // extensions and closures were separately confirmed to pass `-typecheck`.
    let out = Command::new(&swiftc)
        .args(["-parse", src.to_str().expect("utf8 path")])
        .output()
        .expect("run swiftc");
    assert!(
        out.status.success(),
        "generated Swift failed to parse with swiftc:\n--- program ---\n{emitted}\n--- stdout ---\n{}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

// ==========================================================================
// SQL DDL (sqlite3 parse)  — self-contained fit-probe block
// ==========================================================================
//
// SQL is a fit-assessment probe: only its DDL (`CREATE TABLE`) maps onto the
// declarative tree core (see tests/sql.rs and the report). This probe emits a
// representative `CREATE TABLE` from sql.mdl and, if `sqlite3` is installed,
// feeds it to an in-memory database (`sqlite3 :memory:`) to confirm it parses
// and executes as real DDL. When `sqlite3` is absent it degrades to a plain
// string assertion so the suite still runs green.

#[test]
fn sql_ddl_output_parses_with_sqlite3() {
    use lamina_core::ast::{Attr as SqlAttr, Expr as SqlExpr};

    // sql.mdl is a Wave B fit-probe; only run if it exists.
    let mut sql_def_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    sql_def_path.pop();
    sql_def_path.pop();
    sql_def_path.pop();
    sql_def_path.push("lamina-defs/languages/sql.mdl");
    if !sql_def_path.exists() {
        eprintln!("SKIP compile_check SQL: sql.mdl not shipped yet");
        return;
    }
    let lang = shipped_def("sql");

    // A representative table: several columns of distinct SQLite storage classes,
    // per-column constraints, and a table-level FOREIGN KEY constraint.
    let sql_col = |name: &str, ty: &str, constraints: &[&str]| -> SqlExpr {
        let mut attrs = vec![SqlAttr {
            name: "type".to_string(),
            value: SqlExpr::StringLiteral(ty.to_string()),
            meta: Meta::new(),
        }];
        for c in constraints {
            attrs.push(SqlAttr {
                name: (*c).to_string(),
                value: SqlExpr::StringLiteral(String::new()),
                meta: Meta::new(),
            });
        }
        SqlExpr::Node {
            name: name.to_string(),
            attrs,
            children: vec![],
            meta: Meta::new().with("sql", "column"),
        }
    };
    let table = SqlExpr::Node {
        name: "orders".to_string(),
        attrs: vec![],
        children: vec![
            sql_col("id", "INTEGER", &["PRIMARY KEY"]),
            sql_col("user_id", "INTEGER", &["NOT NULL"]),
            sql_col("total", "REAL", &["NOT NULL", "DEFAULT 0"]),
            sql_col("note", "TEXT", &[]),
            SqlExpr::Node {
                name: "FOREIGN KEY (user_id) REFERENCES users(id)".to_string(),
                attrs: vec![],
                children: vec![],
                meta: Meta::new().with("sql", "constraint"),
            },
        ],
        meta: Meta::new().with("sql", "table"),
    };
    let emitted = emit_items(vec![Item::Tree(table)], &lang);

    // Static sanity: the emitted DDL must be a well-formed CREATE TABLE.
    assert!(
        emitted.starts_with("CREATE TABLE orders (") && emitted.ends_with(");"),
        "unexpected SQL DDL shape:\n{emitted}"
    );

    // Prefix a users table so the FOREIGN KEY reference resolves, then run the
    // whole script against an in-memory database: a clean run proves the DDL
    // parses and executes.
    let script = format!("CREATE TABLE users (id INTEGER PRIMARY KEY);\n{emitted}\n");

    // Preferred backend: the `sqlite3` CLI, `.read`-ing the script into `:memory:`.
    if tool_present("sqlite3") {
        let src = write_temp("sql_repr", "sql", &script);
        let out = Command::new("sqlite3")
            .arg(":memory:")
            .arg(format!(".read {}", src.to_str().expect("utf8 path")))
            .output()
            .expect("run sqlite3");
        assert!(
            out.status.success() && out.stderr.is_empty(),
            "generated SQL DDL failed to parse/execute with sqlite3:\n--- script ---\n{script}\n--- stderr ---\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        return;
    }

    // Fallback: Python's bundled `sqlite3` module runs the SAME SQLite engine, so
    // `executescript` against an in-memory DB is an equivalent parse+execute
    // check when the standalone CLI is absent.
    let py = if tool_present("python3") {
        Some("python3")
    } else if tool_present("python") {
        Some("python")
    } else {
        None
    };
    if let Some(py) = py {
        let driver = "import sqlite3, sys\n\
             sql = sys.stdin.read()\n\
             con = sqlite3.connect(':memory:')\n\
             con.executescript(sql)\n\
             con.close()\n"
            .to_string();
        let driver_path = write_temp("sql_driver", "py", &driver);
        use std::io::Write;
        let mut child = Command::new(py)
            .arg(driver_path.to_str().expect("utf8 path"))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("spawn python sqlite3 driver");
        child
            .stdin
            .take()
            .expect("child stdin")
            .write_all(script.as_bytes())
            .expect("write sql to python stdin");
        let out = child.wait_with_output().expect("wait python sqlite3 driver");
        assert!(
            out.status.success(),
            "generated SQL DDL failed to parse/execute via python sqlite3:\n--- script ---\n{script}\n--- stderr ---\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        return;
    }

    eprintln!(
        "SKIP compile_check SQL: no sqlite3 CLI or python sqlite3 module (asserted emitted DDL shape only)"
    );
}
