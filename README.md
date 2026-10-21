# mini-c

A small, embeddable Rust frontend for a deliberately limited subset of C, modeled in part on [bingundu/oberon](https://github.com/bingundu/oberon).

## Implemented functionality

- Incremental lexer and translation-unit parser: each global declaration or complete function body can be consumed independently, without retaining an entire translation-unit AST.
- C subset including signed/unsigned integer types, pointers, function definitions/prototypes (including unnamed prototype parameters), block typedefs, static/extern declarations, arithmetic, comparisons, assignments, calls, blocks, if/else and while.
- Independent `mini-c-sema` crate checks types, lexical scopes, linkage, integer conversions, lvalues, function calls, returns, constant initializers and loop-control legality. Typed expressions include explicit load/conversion nodes, suitable for later backends.
- `mini-c check` now validates **syntax and semantics**, reports filename and source location, and supports external `cc -E`/`clang -E` or already-preprocessed input.
- Deterministic defined-behavior C corpus generator (`mini-c-fuzz`) with an independent arithmetic oracle, reference validation against GCC/Clang, sanitizer smoke tests, failure-artifact capture, and an extended weekly GitHub Actions workflow.

**This is not a conforming ISO C implementation.** Its parser recognizes only a subset; full C declarators, many types, complete language semantics, internal preprocessing, precise expression-level diagnostics, and code generation remain incomplete or future work. C23 provides the language design reference ([WG14 N3220](https://www.open-std.org/jtc1/sc22/wg14/www/docs/n3220.pdf)).

## Development and smoke tests

```sh
cargo test --workspace
cargo test -p mini-c-sema
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p mini-c -- check --cpp=none example.c
cargo run -p mini-c -- check --cpp=cc example.c
cargo run -q -p mini-c-fuzz -- 42 120 > case.c
cargo build -p mini-c-fuzz
python3 tools/fuzz_reference.py --seed-count 12 --steps 120
```

For a preprocessed file, `--cpp=none` skips external tools. For ordinary C sources, use `--cpp=cc`, `--cpp=clang`, or `--cpp=/path/to/preprocessor`; additional preprocessor options use `--cpp-arg=<argument>`. The parser API itself never invokes a preprocessor.

## Project documents

- [Language and architecture specification](docs/Specification.md)
- [Implementation plan and milestones](docs/Implementation_Plan.md)
- [Testing and CI strategy](docs/Testing.md)
- [Defined-behavior fuzzer and reproducibility](docs/Fuzzing.md)
- [Typed frontend API, target model, and limitations](docs/Semantics.md)

[Targeted CI](.github/workflows/syntax.yml) runs syntax, preprocessing, driver, Clippy, and compiler-reference fuzzer checks. [Extended fuzz CI](.github/workflows/fuzz.yml) runs weekly or by manual dispatch. The Rust formatting gate is currently disabled by request.

## Embedding in bingundu/compiler

Mini-C is designed as reusable Rust library crates, not just a standalone compiler. See the [embeddable frontend contract](docs/Specification.md#embeddable-frontend-contract) and [integration acceptance gates](docs/Implementation_Plan.md#modular-integration-acceptance-gate). The compiler driver should depend on Mini-C's frontend libraries and provide the adapter to its existing `compiler-ir`; Mini-C's syntax and semantic crates remain backend-independent.

## Updating the expression grammar

The grammar generator is optional at build time, and the checked-in parser now
uses the handwritten C lexer through LALRPOP's external-token interface.
Normal `cargo test` therefore no longer resolves regex-automata. The small
`lalrpop-util` parser runtime remains required.



The main incremental C parser is handwritten and unchanged. The separate
LALRPOP expression grammar remains editable in
`crates/syntax/src/grammar.lalrpop`. Normal builds use the checked-in
`crates/syntax/src/grammar.rs`, without compiling LALRPOP. After editing:

```sh
cargo run --manifest-path tools/parser-codegen/Cargo.toml -- --write
cargo run --manifest-path tools/parser-codegen/Cargo.toml -- --check
cargo test -p mini-c-syntax -p mini-c-sema
```

Commit both grammar files together. The generator is in an independent
Cargo workspace and is only built on explicit regeneration or CI checks.
