# M2: checked Mini-C frontend

Mini-C's semantic checker is the separate **mini-c-sema** Rust crate. It depends
on the syntax API only, and has no process, filesystem, backend or global-state
dependencies. The parser yields one `ExternalItem` at a time, and `Checker`
returns a distinct `CheckedItem`, which a consumer can lower or discard. The
checker retains file-level declarations/typedef types and their linkage,
but **does not retain previous function ASTs or checked function bodies**.

## Embedding

```rust
use mini_c_syntax::parser::Parser;
use mini_c_sema::{check_stream, Checker, CheckedItem, Target};

let mut parser = Parser::new("int x; int add(int n) { return n + x; }");
let mut checker = Checker::new(Target::default())?;
check_stream(&mut parser, &mut checker, |item| {
    if let CheckedItem::Function(function) = item {
        // Lower this checked function, then drop it before reading the next.
        drop(function);
    }
    Ok(())
})?;
```

`Checker::new` accepts a caller-supplied `Target { char_is_signed,
int_bits, long_bits }` (default: LP64 with signed `char`). Type checking
supports signed and unsigned `char`, `int` and `long`, integer promotions,
usual arithmetic conversions, pointer-compatible assignments and null pointer
constants. The target currently assumes 8-bit `char` and two's-complement
integer conversion; invalid target widths are rejected.

## Checked representation

- `CheckedExpr { ty, category, kind }` records value vs modifiable lvalue vs
  function designator and inserts explicit `Load` and `Convert` nodes.
- `CheckedStmt` contains converted and checked initializers, typed conditions,
  returns, and recursively checked blocks.
- `CheckedFunction` contains a typed signature, typed parameters, storage
  linkage, and one checked body.
- `CheckedDecl` records type, storage class, linkage, initializer and span.
- `Checker` holds persistent file-scope object/function/typedef symbols and
  temporary scopes only during the current function.

File scope supports repeated compatible prototypes and tentative object
declarations, but rejects conflicting types, duplicate function definitions,
duplicate initialized global definitions, and incompatible linkage. `extern`
declarations may inherit prior internal linkage; `static` objects/functions
have internal linkage at file scope. Local static objects keep storage-class
metadata and require a constant initializer; block-scope `extern` refers to a
compatible file-scope symbol. Parameters share the scope of the function's
outermost block; nested blocks may shadow names.

### Current validation coverage

- Unknown variables/functions, typedefs used as expressions, duplicate
  parameters and local names, incompatible global declarations.
- Modifiable lvalue required for assignment; pointer dereference and
  address-of constraints; integer arithmetic; scalar logical conditions.
- Exact argument count and assignment-compatible argument/result types.
- Valid `void`/non-void return statements and `break`/`continue` only in loops.
- Integer static-initializer expressions evaluated using **typed target
  widths**; signed overflow and division by zero rejected. C `&&` and `||`
  are short-circuited. Static pointer initializers accept a null integer
  constant or address of a declared global object/function.

Use `cargo test -p mini-c-sema` for the focused positive/compile-fail suite.
`cargo test --workspace` and Clippy are part of the cached syntax workflow.
The CLI `cargo run -p mini-c -- check [--cpp=none|--cpp=cc] file.c` now checks
both syntax and semantics and returns a nonzero exit status on either error.

## Deliberate limits and next stages

This is a **restricted C subset**, not ISO C23 conformance. The syntax layer
does not yet support full declarators, casts, structures, floating point,
array operations, string literals, variadic or old-style C prototypes, enums
or compound literals. The checked frontend does not yet implement complete
declaration/storage-duration/linkage subtleties, signed-`char` vs plain-`char`
distinct type identity, pointer arithmetic, complex constant expressions,
constant-address arithmetic, or all standard integer ranks. These features
are deliberately rejected where unsupported rather than silently lowered.

The syntax AST currently carries precise spans for declarations/functions,
but expression nodes lack their own byte spans. Semantic errors in expressions
therefore use the enclosing function or declaration span. **M2.25 must add
expression/statement-level spans, diagnostic recovery and stable diagnostic
codes**, without retaining entire translation-unit ASTs. M2.5 adds an
independent Cranelift executable backend; the checked IR itself contains no
Cranelift or native-backend dependency.
