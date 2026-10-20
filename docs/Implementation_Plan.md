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

## M2 — Typed C frontend

Implement lexical scopes, declaration linkage/storage classes, signedness and integer conversions, lvalues, calls, constant expressions, return checking and control-flow legality. Keep a distinct checked representation. Add precise error spans and compile-fail cases.

Exit: valid integer programs type-check; invalid programs fail with stable diagnostics.

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

Immediate order: fix M0 compilation; implement M1 grammar and typedef lexer; M2 type checking; M3 backend integration. External preprocessor interface can land alongside M1, while full M6 coverage follows native execution. At every step prefer targeted tests; run full workspace/integration suites only on milestone completion or significant IR/backend changes.

## Cross-repository coordination

Mini-C owns syntax/sema/lowering. `bingundu/compiler` owns shared IR, optimizations and native backend. `bingundu/oberon` remains an architecture and differential-testing reference. Changes to shared IR require isolated compiler crate tests first, then Mini-C integration tests; document revision pins. Avoid cyclic Git dependencies.

## Modular integration acceptance gate

- M0: syntax builds as a library with no CLI or compiler dependency.
- M1: parsing entry point takes source text and returns AST/diagnostics without filesystem or process side effects.
- M2: checker exposed as a library; target/layout supplied by caller; no global symbol table.
- M3: add a separate adapter in `bingundu/compiler` that consumes checked Mini-C functions and produces `compiler-ir`; keep Mini-C core crates backend-neutral. Add a compiler-driver smoke test selecting C input and reusing existing optimizer/backend.
- M4-M7: expand bridge coverage while keeping standalone frontend tests independent of backend and CI secrets.

Before completing M3, verify Cargo dependency graph is acyclic and test the embedded API directly from a separate integration crate. Do not put command-line parsing or external `cc -E` invocation in syntax/sema crates.
