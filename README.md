# mini-c

A small, embeddable Rust frontend for a deliberately limited subset of C, modeled in part on [bingundu/oberon](https://github.com/bingundu/oberon).

**C99 frontend priorities:** The active source-audited roadmap is [C99 Frontend Plan](docs/C99_Frontend_Plan.md). F1 and practical F2/F3/F4 work continue; **F5 advanced numeric/lexical work is deferred**, while completing the internal C99 preprocessor is an active P0 milestone ([CPP-C99 #110](https://github.com/bingordo/mini-c/issues/110), slices #111–#114). Parser/semantic C99 coverage and CPP conformance are tracked independently of native and Cranelift codegen.

## Implemented functionality

- Incremental lexer and translation-unit parser: each global declaration or complete function body can be consumed independently, without retaining an entire translation-unit AST.
- C subset including signed/unsigned integer types, pointers, function definitions/prototypes (including unnamed prototype parameters), block typedefs, static/extern declarations, arithmetic, comparisons, assignments, calls, blocks, if/else and while.
- Recursive C declarator parsing handles pointer/array binding, grouped declarators, function-pointer typedefs, and abstract parameter declarators. Semantic analysis now represents fixed-size and incomplete array types, checks integer constant bounds, adjusts array/function parameters to pointers, and performs array-to-pointer decay. Array indexing, checked pointer+integer offsets and indexed assignments now have typed lvalue semantics. Native memory lowering, aggregate initialization and full ABI handling remain incomplete.
- Independent `mini-c-sema` crate checks types, lexical scopes, linkage, integer conversions, lvalues, function calls, returns, constant initializers and loop-control legality. Typed expressions include explicit load/conversion nodes, suitable for later backends.
- `mini-c check` validates **syntax and semantics** and normally runs external `cc -E`. It supports `-I`/`-D`/`-U`, `--cpp=clang` or a custom preprocessor, `--cpp=none` for already-preprocessed text, and diagnostics remapped through include-file line markers.
- Deterministic defined-behavior C corpus generator (`mini-c-fuzz`) with an independent arithmetic oracle, reference validation against GCC/Clang, sanitizer smoke tests, failure-artifact capture, and an extended weekly GitHub Actions workflow.
- An **optional Cranelift ELF object backend** (`mini-c-cranelift`, M2.5/M5) compiles supported scalar/pointer functions, globals, strings, structures, unions, enums, aggregates and cross-object references; by-value SysV struct/union calls remain explicitly unsupported. It shares the same external preprocessing options as the checker.

**This is not a conforming ISO C implementation.** Its parser recognizes only a subset; full C declarators, many types, complete language semantics, internal preprocessing, precise expression-level diagnostics, and the full native backend remain incomplete or future work. C23 provides the language design reference ([WG14 N3220](https://www.open-std.org/jtc1/sc22/wg14/www/docs/n3220.pdf)).

**Future Csmith differential tests:** [Csmith end-to-end milestone #116](https://github.com/bingordo/mini-c/issues/116) tracks unchanged source-pinned Csmith programs through external/internal preprocessing, checked frontend, native object/link/run and seeded differential CI. The existing real-source floating-helper blocker is [#99](https://github.com/bingordo/mini-c/issues/99). These tests are planned *after* immediate practical C99 and internal-CPP milestones; compiler acceptance must be demonstrated by actual execution, not merely parsing.

## Development and smoke tests

```sh
cargo test -p mini-c-syntax -p mini-c-preprocess -p mini-c-sema -p mini-c -p mini-c-fuzz
cargo clippy -p mini-c-syntax -p mini-c-sema -p mini-c -p mini-c-fuzz --all-targets -- -D warnings
cargo run -p mini-c -- check --cpp=none example.c
cargo run -p mini-c -- check example.c    # default: cc -E
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

Both drivers default to external `cc -E` preprocessing. `--cpp=none`
bypasses all subprocesses for already-preprocessed sources; CPP flags cannot
be combined with that mode. `--cpp=clang` and `--cpp=/path/to/tool` select
other executables. `-I`, `-D`, `-U` accept attached or separate values, and
`--cpp-arg=<argument>` passes an additional argument verbatim. Drivers pass
arguments directly to subprocesses, not via a shell. Diagnostics restore the
original filename and line using preprocessor markers (macro-expansion columns
cannot be reconstructed). See [M6 preprocessing](docs/M6_Preprocessing.md).

**Experimental internal CPP:** Mini-C also has a feature-gated C99
preprocessing adapter that does not launch an external compiler or embed
another C frontend. It reuses only a pinned Rust preprocessing engine;
GCC/Clang remain the conformance references. Try:

```sh
cargo run -p mini-c --features internal-cpp -- check --cpp=internal -I include -DFEATURE=1 example.c
cargo run -p mini-c-cranelift --features internal-cpp -- --emit-obj /tmp/example.o --cpp=internal example.c
```

The internal option is **not enabled by default** and is not yet ISO C99
preprocessing-complete. [CPP-C99 #110](https://github.com/bingordo/mini-c/issues/110)
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
