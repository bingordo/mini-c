# Unified Mini-C implementation plan

This plan combines language scope, architecture, backend reuse and validation. See [Specification](Specification.md) and [CI and testing](Testing.md). Work in substantial vertical slices; run targeted tests on every change and full integration suites at milestones.

## M0 — Foundation (partially complete)

- [x] Rust workspace and syntax AST scaffold.
- [x] LALRPOP expression grammar scaffold and precedence tests (not yet executed).
- [x] Scoped typedef/ordinary identifier table (not yet connected to lexer).
- [ ] Make workspace compile and pass syntax tests; add parser diagnostics and source spans.
- [ ] Document C23 subset and configure CI targeted checks.

Exit: reproducible green `cargo test -p mini-c-syntax` and `cargo clippy -p mini-c-syntax --all-targets -- -D warnings`.

## M1 — Usable C parser

Implement a stateful lexer, translation-unit grammar, declarators, function definitions/prototypes, statements and expression precedence; preserve source locations. Wire typedef-aware lexing to scope changes at declaration points and verify parser lookahead behavior. Add malformed-input and ambiguity regressions. Build a small `mini-c check` command with clear diagnostics.

Exit: parse and reject a documented integer-C corpus; typedef shadowing and nested scopes correct.

## M1.25 — External preprocessing and source mapping

Add a separate preprocessing/driver integration layer immediately after M1. Accept raw C sources through a configurable external C preprocessor (`cc -E` or `clang -E`) and accept already-preprocessed inputs through `--cpp=none`. Keep preprocessing outside `mini-c-syntax` and semantic-analysis crates: library parsing must work directly on caller-provided source text. Invoke tools using argument vectors, never shell interpolation; preserve exit status, stderr, and executable path in diagnostics. Make executable and flags configurable; do not assume a specific host compiler is installed for syntax unit tests.

Consume standard preprocessor line markers to map diagnostics back to original filenames and line numbers, including includes. Retain generated-source spans and a separate origin map; avoid silently claiming byte-exact columns when external macro expansion makes them unknowable. Test paths with spaces, failed preprocessing, nested includes, conditional compilation, predefined macros, and `--cpp=none` equivalence. Integration tests requiring a host compiler should be isolated from core unit tests.

Exit: reproducible end-to-end parsing of preprocessed translation units, accurate file/line diagnostics for ordinary includes, and a documented preprocessor-off path. Internal preprocessing remains M6.

## M1.5 — Defined-behavior C fuzzer and independent oracle

Create a deterministic, seed-reproducible C program generator inspired by `bingundu/oberon`'s `obgen`, before semantic analysis and code generation are complete. Generate only constructs currently supported by Mini-C, initially integer expressions and simple functions; extend to blocks, branches, loops, calls and arrays as milestones land. Generate expected outputs with an **independent evaluator** (not the compiler's AST evaluator, optimizer or backend) and emit self-checking C programs plus seed, generator version, and repro command. Use bounded execution/fuel for loops and recursion; avoid nontermination.

**Defined behavior by construction:** use fixed known types and ranges, checked arithmetic in the generator; exclude signed overflow, division/remainder by zero, `INT_MIN / -1`, invalid shifts (negative or >= width), left shifts of negative or overflowing signed values, uninitialized reads, invalid pointers, out-of-bounds access, lifetime violations, strict-aliasing violations, unsequenced conflicting accesses, unspecified evaluation-order dependencies, and implementation-defined conversions/representations unless deliberately constrained to a documented target. Avoid `-fwrapv` as a substitute for valid ISO C semantics. Do not accidentally assume signed wraparound. Prefer unsigned arithmetic for intentionally modular operations, and explicit bounds for signed operations. Emit only initialized data, and prove indices/ranges at generation time. Keep an auditable generation rule catalog describing each invariant.

Compile generated programs with GCC and Clang (when available), at `-O0` and `-O2`, with UBSan/ASan reference runs where applicable. Compare with independent expected values and persist every failure seed as a regression. Run a small deterministic smoke set on each relevant PR and larger seeded campaigns manually/weekly. Include negative tests for the generator itself (overflow/invalid shift/division guards), and a review of whether the oracle accidentally shares implementation logic with Mini-C.

Exit: stable deterministic seeds, an independently computed oracle, sanitizer-clean GCC/Clang reference executions, documented defined-behavior invariants and an automated smoke corpus. Expand the corpus with each subsequent milestone.

## M2 — Typed C frontend

Implement lexical scopes, declaration linkage/storage classes, signedness and integer conversions, lvalues, calls, constant expressions, return checking and control-flow legality. Keep a distinct checked representation. Add precise error spans and compile-fail cases.

Exit: valid integer programs type-check; invalid programs fail with stable diagnostics.

## M2.25 — Syntax and semantic diagnostic quality

Build a focused error-handling test suite and improve parser/checker diagnostics before executable backends. Cover invalid tokens, missing delimiters, malformed declarators, typedef ambiguity and shadowing, unexpected EOF, duplicate declarations, unknown identifiers, incompatible types, invalid lvalues, wrong argument counts, invalid returns, and scope errors. Verify exact source spans (including UTF-8, multi-line input and preprocessed source mapping), actionable messages, stable diagnostic ordering, and absence of panics. Prefer useful recovery at statement/declaration boundaries so a single source can produce multiple independent errors without cascading noise; impose a configurable error limit. Make error reporting deterministic and available through the embeddable library API without printing or exiting.

Testing: table-driven positive/negative fixtures with expected diagnostic codes, primary spans and optional notes; malformed-input mutation/fuzz tests; regression tests for every reported crash; snapshot tests with deliberate review of wording changes. Keep tests targeted and fast in PR CI. The CLI may render annotated source, but syntax/sema only return structured diagnostics.

Exit: robust recovery on representative malformed programs, stable diagnostic assertions, no crashes in bounded malformed-input fuzzing, and clear differentiation between syntax and semantic failures.

## M2.5 — Cranelift executable backend (before native compiler integration)

Implement a small, separate Cranelift lowering crate consuming the checked Mini-C representation. Reuse patterns from `bingundu/oberon`'s Cranelift backend, but do not make Mini-C syntax or semantic crates depend on Cranelift or Oberon. Provide a standalone CLI or test harness that compiles supported integer functions to native objects, links them using the system C toolchain, and executes differential tests against GCC. Start with integer arithmetic, parameters, local variables, return, if/else and while; add calls and for loops as the checked frontend supports them. Use the same semantic fixtures as M2 and the eventual M3 backend, and document unsupported features.

Exit: a reproducible green Cranelift executable smoke suite with GCC comparisons, including loops, branches and function calls where implemented. The backend is an optional leaf crate and can be disabled without affecting parser/sema tests. This milestone precedes M3 so it provides an independent executable oracle while the new compiler backend is integrated.

## M3 — First executable native vertical slice

Pin and integrate `bingundu/compiler` IR/optimizer/backend crates. Lower checked int functions, CFG branches/loops, short-circuit operators, calls where supported. Generate ELF64 object or assembly, link with cc and execute against a GCC-built reference. Fix backend gaps in the compiler repository, not by adding a parallel code generator.

Exit: integer arithmetic/control-flow corpus executes with expected results on x86-64 SysV.

## M4 — C memory model essentials

Add arrays, pointers, address-of/dereference, pointer arithmetic, alignment, conversions and initialized locals. Extend IR and backend for required 64-bit address operations and memory effects. Test aliasing, uninitialized-value diagnostics where feasible, and sanitizers on reference executions.

Exit: pointer/array programs link and run correctly, including stack spills.

## M5 — Linkage and aggregates

Add global variables, external calls, ELF symbol/relocation support, strings, structs/unions/enums and layout; implement supported SysV aggregate ABI cases or reject unsupported ones. Test multi-file linkage with cc and mixed compiler/GCC objects.

Exit: cross-translation-unit tests and ABI probes pass.

## M6 — Preprocessor maturity

Integrate external preprocessing with -I/-D/-U and source-line remapping; support --cpp=none. Optionally implement an internal restricted preprocessor behind explicit selection; differential-test against GCC/Clang. Do not block M1-M3 on internal macros.

Exit: reproducible include/macro fixtures, correct diagnostics, no shell injection.

## M7 — Differential hardening

Add seeded program generator with independently computed oracle where possible; compile against GCC and Clang; run optimization modes and debug/release compiler builds. Add negative tests, fuzz regressions, and ABI/ELF validation. Review code and docs at each milestone.

Exit: green scheduled extended CI with reproducible seeds and documented unsupported features.

## Work ordering

Immediate order: fix M0 compilation; implement M1 grammar and typedef lexer; M1.5 defined-behavior fuzzer; M2 type checking; M2.5 Cranelift executable reference; M3 backend integration. External preprocessor interface can land alongside M1, while full M6 coverage follows native execution. At every step prefer targeted tests; run full workspace/integration suites only on milestone completion or significant IR/backend changes.

## Cross-repository coordination

Mini-C owns syntax/sema/lowering. `bingundu/compiler` owns shared IR, optimizations and native backend. `bingundu/oberon` remains an architecture and differential-testing reference. Changes to shared IR require isolated compiler crate tests first, then Mini-C integration tests; document revision pins. Avoid cyclic Git dependencies.

## Modular integration acceptance gate

- M0: syntax builds as a library with no CLI or compiler dependency.
- M1: parsing entry point takes source text and returns AST/diagnostics without filesystem or process side effects.
- M2: checker exposed as a library; target/layout supplied by caller; no global symbol table.\n- M2.5: standalone Cranelift leaf crate consumes checked functions without introducing a dependency from syntax/sema to Cranelift.
- M3: add a separate adapter in `bingundu/compiler` that consumes checked Mini-C functions and produces `compiler-ir`; keep Mini-C core crates backend-neutral. Add a compiler-driver smoke test selecting C input and reusing existing optimizer/backend.
- M4-M7: expand bridge coverage while keeping standalone frontend tests independent of backend and CI secrets.

Before completing M3, verify Cargo dependency graph is acyclic and test the embedded API directly from a separate integration crate. Do not put command-line parsing or external `cc -E` invocation in syntax/sema crates.

## Incremental compilation contract

The production compiler consumes input incrementally: preprocessing output through a reader or pipe, tokens on demand, and top-level declarations one at a time. A complete AST may be materialized for **one function at a time**, passed to semantic analysis and lowering, and discarded after compilation. Durable state consists of file-scope symbols, types, linkage, diagnostics, and necessary object-file/relocation metadata, not completed function ASTs. Peak memory should not scale with the number of completed function bodies.

Typedef-sensitive lexing must classify names at token consumption time; parser lookahead must not permanently classify future identifiers across declaration or scope updates. The existing TranslationUnit AST remains a convenience for tests, not the production driver contract. The external preprocessor must eventually stream stdout instead of collecting its entire output. Tests should cover many functions and typedef shadowing at scope boundaries.

A future inliner may optionally retain selected compact function IR bodies under a memory budget, but **no inliner is part of the current milestones**. Independent function compilation remains the default.
