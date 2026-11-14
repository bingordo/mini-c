# mini-c

A small, embeddable Rust frontend for a deliberately limited subset of C, modeled in part on [bingundu/oberon](https://github.com/bingundu/oberon).

**C99 frontend priorities:** The active source-audited roadmap is [C99 Frontend Plan](docs/C99_Frontend_Plan.md). **F1–F4 practical C frontend functionality is the active priority**, including switch/_Bool and real-source expression/type support. **F5 advanced numeric/lexical work remains deferred.** The internal preprocessor is the pragmatic driver default after [#138](https://github.com/bingordo/mini-c/pull/138); further CPP-C99 conformance/resource work under [#110](https://github.com/bingordo/mini-c/issues/110)–#114 is nonblocking follow-up. Parser/semantic C99 coverage and CPP conformance are tracked independently of native and Cranelift codegen.

## Implemented functionality

- Incremental lexer and translation-unit parser: each global declaration or complete function body can be consumed independently, without retaining an entire translation-unit AST.
- C subset including signed/unsigned integer types, pointers, function definitions/prototypes (including unnamed prototype parameters), block typedefs, static/extern declarations, arithmetic, comparisons, assignments, calls, blocks, if/else and while.
- Recursive C declarator parsing handles pointer/array binding, grouped declarators, function-pointer typedefs, and abstract parameter declarators. Semantic analysis now represents fixed-size and incomplete array types, checks integer constant bounds, adjusts array/function parameters to pointers, and performs array-to-pointer decay. Array indexing, checked pointer+integer offsets and indexed assignments now have typed lvalue semantics. Native memory lowering, aggregate initialization and full ABI handling remain incomplete.
- C99 `switch`/`case`/`default`: parser and semantic support for integer promotions, constant cases, duplicates, nested labels, fallthrough and switch-aware breaks. The Cranelift reference backend handles common direct-case dispatch and nested switches, validated by six GCC-matching executable cases; arbitrary nested label placements remain a tracked backend gap ([#143](https://github.com/bingordo/mini-c/issues/143)).
- Independent `mini-c-sema` crate checks types, lexical scopes, linkage, integer conversions, lvalues, function calls, returns, constant initializers and loop-control legality. Typed expressions include explicit load/conversion nodes, suitable for later backends.
- `mini-c check` validates **syntax and semantics** and runs the built-in C preprocessor by default. It supports `-I`/`-D`/`-U`, `--cpp=clang` or a custom preprocessor, `--cpp=none` for already-preprocessed text, and diagnostics remapped through include-file line markers.
- Deterministic defined-behavior C corpus generator (`mini-c-fuzz`) with an independent arithmetic oracle, reference validation against GCC/Clang, sanitizer smoke tests, failure-artifact capture, and an extended weekly GitHub Actions workflow.
- An **optional Cranelift ELF object backend** (`mini-c-cranelift`, M2.5/M5) compiles supported scalar/pointer functions, globals, strings, structures, unions, enums, aggregates and cross-object references. Its tested x86-64 SysV ABI includes scalar `float`/`double`, INTEGER/SSE register-class structs, MEMORY-class by-value struct calls and hidden structure returns, plus direct scalar floating-point variadics. Aggregate copies currently have a 4,096-byte limit; indirect variadics and extended FP are unsupported. See [Cranelift constraints](docs/Cranelift.md). It shares the internal-default and external-fallback preprocessing options with the checker.

**This is not a conforming ISO C implementation.** Its parser recognizes only a subset; full C declarators, many types, complete language semantics, internal preprocessing, precise expression-level diagnostics, and the full native backend remain incomplete or future work. C23 provides the language design reference ([WG14 N3220](https://www.open-std.org/jtc1/sc22/wg14/www/docs/n3220.pdf)).

**Future Csmith differential tests:** [Csmith end-to-end milestone #116](https://github.com/bingordo/mini-c/issues/116) tracks unchanged source-pinned Csmith programs through external/internal preprocessing, checked frontend, native object/link/run and seeded differential CI. The existing real-source floating-helper blocker is [#99](https://github.com/bingordo/mini-c/issues/99). These tests are planned *after* immediate practical C99 and internal-CPP milestones; compiler acceptance must be demonstrated by actual execution, not merely parsing.

## Development and smoke tests

```sh
cargo test -p mini-c-syntax -p mini-c-preprocess -p mini-c-sema -p mini-c -p mini-c-fuzz
cargo clippy -p mini-c-syntax -p mini-c-sema -p mini-c -p mini-c-fuzz --all-targets -- -D warnings
cargo run -p mini-c -- check --cpp=none example.c
cargo run -p mini-c -- check example.c    # default: internal C preprocessor
cargo run -p mini-c -- check --cpp=cc example.c    # explicit external fallback
cargo run -p mini-c -- check -I include -DDEBUG=1 -UOLD example.c
cargo run -p mini-c -- check --cpp=clang example.c
cargo run -p mini-c -- check --max-errors=8 example.c
cargo run -q -p mini-c-fuzz -- 42 120 > case.c
cargo build -p mini-c-fuzz
python3 tools/fuzz_reference.py --seed-count 12 --steps 120

# Optional Cranelift backend (separate, heavier dependency closure):
cargo test -p mini-c-cranelift
cargo build -p mini-c-cranelift -p mini-c-fuzz
cargo run -p mini-c-cranelift -- --emit-obj /tmp/example.o -I include -DDEBUG=1 example.c
cc /tmp/example.o -o /tmp/example
python3 tools/test_cranelift.py --seed-count 4
```

Both drivers now **default to the internal preprocessor** with ordinary
`cargo build` and `cargo run` (the `internal-cpp` Cargo feature is enabled by
default for both binary crates). `--cpp=cc` explicitly selects external
`cc -E` for compatibility or investigation, while `--cpp=clang` or
`--cpp=/path/to/tool` select other executable preprocessors. `--cpp=none`
bypasses preprocessing entirely for already-preprocessed sources; CPP flags
cannot be combined with that mode. `-I`, `-D`, `-U` accept attached or separate values, and
`--cpp-arg=<argument>` passes an additional argument verbatim. Drivers pass
arguments directly to subprocesses, not via a shell. Diagnostics restore the
original filename and line using preprocessor markers (macro-expansion columns
cannot be reconstructed). See [M6 preprocessing](docs/M6_Preprocessing.md).

**Internal CPP (default in the Mini-C binaries):** Mini-C uses a Rust C99
preprocessing adapter that does not run an external preprocessor or embed
another C frontend. GCC/Clang remain the conformance references. For example:

```sh
cargo run -p mini-c -- check -I include -DFEATURE=1 example.c
cargo run -p mini-c-cranelift -- --emit-obj /tmp/example.o example.c
# To investigate a disagreement, force the old external path:
cargo run -p mini-c -- check --cpp=cc example.c
```

The internal engine is the **default for practical Mini-C driver use**, but
it is not yet a fully ISO C99-conforming preprocessor. Build with
`--no-default-features` if a smaller external-only binary is needed; such
builds default back to `cc -E` and cannot select `--cpp=internal`.
The independent `mini-c-preprocess` library still defaults to an external
configuration when built without the optional engine; it does not force
C parser or semantic crates to depend on vendored preprocessing. [CPP-C99 #110](https://github.com/bingordo/mini-c/issues/110)
tracks C99 macro/phase parity, target built-ins and include paths, source
provenance, bounded expansion and final CI acceptance; C23 is deferred. See
[Internal CPP Prototype](docs/Internal_CPP_Prototype.md) for current implementation
and limitations.

The parser API itself never invokes a preprocessor. The default checker CLI
stops at the first error; passing `--max-errors=N` enables bounded syntax and
semantic recovery across external declarations and prints stable error
category codes. This does not yet recover multiple independent errors inside
one function.

## Project documents

- [Language and architecture specification](docs/Specification.md)
- [Implementation plan and milestones](docs/Implementation_Plan.md)
- [Testing and CI strategy](docs/Testing.md)
- [Defined-behavior fuzzer and reproducibility](docs/Fuzzing.md)
- [Typed frontend API, target model, and limitations](docs/Semantics.md)
- [Cranelift ELF backend, constraints, and executable tests](docs/Cranelift.md)
- [M5 aggregate layout and ABI closure](docs/M5_Aggregates_ABI.md)
- [M6 external preprocessing interface and tests](docs/M6_Preprocessing.md)

[Syntax CI](.github/workflows/syntax.yml) covers frontend and reference-fuzzer checks when native account minutes permit; [extended fuzz CI](.github/workflows/fuzz.yml) has deeper scheduled tests. Active scheduled native validation is offloaded to [bingordo/compiler-ci: Mini-C Cranelift](https://github.com/bingordo/compiler-ci/actions/workflows/mini-c-cranelift.yml) every 15 minutes, skipping unchanged revisions after successful validation. It tests the preprocessor, both drivers, Clippy, ELF emission and GCC differential execution. The full workspace suite remains available there by manual dispatch with the `full_workspace` option. The Rust formatting gate is currently disabled by request.

## Embedding in bingundu/compiler

Mini-C is designed as reusable Rust library crates, not just a standalone compiler. See the [embeddable frontend contract](docs/Specification.md#embeddable-frontend-contract) and [integration acceptance gates](docs/Implementation_Plan.md#modular-integration-acceptance-gate). The compiler driver should depend on Mini-C's frontend libraries and provide the adapter to its existing `compiler-ir`; Mini-C's syntax and semantic crates remain backend-independent.

## Updating the expression grammar

The grammar generator is optional at build time, and the checked-in parser now
uses the handwritten C lexer through LALRPOP's external-token interface.
Normal `cargo test` therefore no longer resolves regex-automata. The small
`lalrpop-util` parser runtime remains required. In version 0.23.1, even
enabling `lalrpop-util/std` pulls `regex-automata` back into the graph,
so the dependency uses `default-features = false` with no extra features.
The syntax CI checks the normal Cargo dependency closure.



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
