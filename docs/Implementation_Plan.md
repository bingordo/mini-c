# Unified Mini-C implementation plan

This plan combines language scope, architecture, backend reuse and validation. See [Specification](Specification.md) and [CI and testing](Testing.md). Work in substantial vertical slices; run targeted tests on every change and full integration suites at milestones.

## M0 — Foundation (complete; latest CI success assumed pending confirmation)

- [x] Rust workspace and syntax AST scaffold.
- [x] LALRPOP expression grammar scaffold and precedence tests.
- [x] Scoped typedef/ordinary identifier table and independent scope-aware scanner (parser integration remains M1).
- [x] Compile syntax workspace, run syntax tests, add parser diagnostics and source spans.
- [x] Document C23 subset and configure CI targeted checks.

Exit: reproducible green `cargo test -p mini-c-syntax` and `cargo clippy -p mini-c-syntax --all-targets -- -D warnings`.

## M1 — Usable C parser

Implement a stateful lexer, translation-unit grammar, declarators, function definitions/prototypes, statements and expression precedence; preserve source locations. Wire typedef-aware lexing to scope changes at declaration points and verify parser lookahead behavior. Add malformed-input and ambiguity regressions. Build a small `mini-c check` command with clear diagnostics.

Exit: parse and reject a documented integer-C corpus; typedef shadowing and nested scopes correct.

## M1.25 — External preprocessing and source mapping (core implementation validated)

Add a separate preprocessing/driver integration layer immediately after M1. The `mini-c check` driver now supports `--cpp=none` (default), `--cpp=cc`, `--cpp=clang`, or a custom executable path; repeat `--cpp-arg=...` for arguments after `-E`. It parses preprocessed output while ignoring line markers and reports mapped file/line (not invented columns). Host-compiler integration tests now cover includes, conditional compilation, macros, paths with spaces, failure propagation, and source-origin mapping. The syntax/preprocessing and CLI test jobs passed in [CI run 39481336472](https://github.com/bingundu/mini-c/actions/runs/39481336472). Nested-include fixtures and unusual line-marker edge cases remain worthwhile follow-ups. Accept raw C sources through a configurable external C preprocessor (`cc -E` or `clang -E`) and accept already-preprocessed inputs through `--cpp=none`. Keep preprocessing outside `mini-c-syntax` and semantic-analysis crates: library parsing must work directly on caller-provided source text. Invoke tools using argument vectors, never shell interpolation; preserve exit status, stderr, and executable path in diagnostics. Make executable and flags configurable; do not assume a specific host compiler is installed for syntax unit tests.

Consume standard preprocessor line markers to map diagnostics back to original filenames and line numbers, including includes. Retain generated-source spans and a separate origin map; avoid silently claiming byte-exact columns when external macro expansion makes them unknowable. Test paths with spaces, failed preprocessing, nested includes, conditional compilation, predefined macros, and `--cpp=none` equivalence. Integration tests requiring a host compiler should be isolated from core unit tests.

Exit: reproducible end-to-end parsing of preprocessed translation units, accurate file/line diagnostics for ordinary includes, and a documented preprocessor-off path. Internal preprocessing remains M6.

## M1.5 — Defined-behavior C fuzzer and independent oracle (complete; CI verified)

The `mini-c-fuzz` crate now implements seed-reproducible checked signed-integer programs, an AST-independent arithmetic trace and oracle, controlled if/else branches, and bounded while loops. The executable emits self-checking C; `tools/fuzz_reference.py` compiles and executes the corpus with GCC/Clang at `-O0`/`-O2`, plus GCC UBSan and Clang ASan+UBSan. The harness retains source, command lines, seed, exit statuses and compiler-version logs on failure. Targeted smoke tests run on every relevant push; `.github/workflows/fuzz.yml` runs longer seeded campaigns weekly or by manual dispatch. [Fuzzing](Fuzzing.md) documents safety rules and reproduction. [CI run 39481336472](https://github.com/bingundu/mini-c/actions/runs/39481336472) passed Rust tests, Python runner tests, both reference compilers and Clippy.

**Remaining as corpus expansion, not M1.5 blockers:** more language constructs (calls, arrays, unsigned arithmetic), persistent minimized regression seeds, and broader scheduled seed sweeps.

Create a deterministic, seed-reproducible C program generator inspired by `bingundu/oberon`'s `obgen`, before semantic analysis and code generation are complete. Generate only constructs currently supported by Mini-C, initially integer expressions and simple functions; extend to blocks, branches, loops, calls and arrays as milestones land. Generate expected outputs with an **independent evaluator** (not the compiler's AST evaluator, optimizer or backend) and emit self-checking C programs plus seed, generator version, and repro command. Use bounded execution/fuel for loops and recursion; avoid nontermination.

**Defined behavior by construction:** use fixed known types and ranges, checked arithmetic in the generator; exclude signed overflow, division/remainder by zero, `INT_MIN / -1`, invalid shifts (negative or >= width), left shifts of negative or overflowing signed values, uninitialized reads, invalid pointers, out-of-bounds access, lifetime violations, strict-aliasing violations, unsequenced conflicting accesses, unspecified evaluation-order dependencies, and implementation-defined conversions/representations unless deliberately constrained to a documented target. Avoid `-fwrapv` as a substitute for valid ISO C semantics. Do not accidentally assume signed wraparound. Prefer unsigned arithmetic for intentionally modular operations, and explicit bounds for signed operations. Emit only initialized data, and prove indices/ranges at generation time. Keep an auditable generation rule catalog describing each invariant.

Compile generated programs with GCC and Clang (when available), at `-O0` and `-O2`, with UBSan/ASan reference runs where applicable. Compare with independent expected values and persist every failure seed as a regression. Run a small deterministic smoke set on each relevant PR and larger seeded campaigns manually/weekly. Include negative tests for the generator itself (overflow/invalid shift/division guards), and a review of whether the oracle accidentally shares implementation logic with Mini-C.

Exit: stable deterministic seeds, an independently computed oracle, sanitizer-clean GCC/Clang reference executions, documented defined-behavior invariants and an automated smoke corpus. Expand the corpus with each subsequent milestone.

## M2 — Typed C frontend (implemented, final CI verification pending)

Implemented in the backend-neutral `mini-c-sema` crate, with `Checker::check_item` and `check_stream` processing one external AST item/function at a time. Syntax supports signed/unsigned integer specifiers, block-scope typedefs, unnamed prototype parameters, function-type typedefs, and `static`/`extern` storage specifiers. Semantic analysis resolves lexical and file scopes, target-configurable signed/unsigned integers and conversions, explicit lvalue-to-rvalue loads, calls and argument coercion, linkage/redeclaration rules, return types, loop-only break/continue, and target-aware static constant expressions. Positive/negative regression tests and `mini-c check` integration tests are implemented. See [Semantics](Semantics.md).

The typed frontend and its initial validation passed [CI run 39492343366](https://github.com/bingundu/mini-c/actions/runs/39492343366). Later improvements to prototype handling and typed constant-expression evaluation require a new complete CI run. Some recent attempts failed **before any runner was assigned** (job runner ID 0, no steps/logs), not at a compilation or test step. Do not mark CI verification complete until a full run executes.

**Deliberate subset boundaries:** full declarators, aggregates, full C storage/linkage details, richer pointer semantics and expression-specific spans remain out of scope. Precise expression spans, error recovery, and stable diagnostic codes belong to M2.25.

Exit: valid supported integer programs type-check; invalid programs reject with stable messages and declaration/function spans.

## M2.25 — Syntax and semantic diagnostic quality

Build a focused error-handling test suite and improve parser/checker diagnostics before executable backends. Cover invalid tokens, missing delimiters, malformed declarators, typedef ambiguity and shadowing, unexpected EOF, duplicate declarations, unknown identifiers, incompatible types, invalid lvalues, wrong argument counts, invalid returns, and scope errors. Verify exact source spans (including UTF-8, multi-line input and preprocessed source mapping), actionable messages, stable diagnostic ordering, and absence of panics. Prefer useful recovery at statement/declaration boundaries so a single source can produce multiple independent errors without cascading noise; impose a configurable error limit. Make error reporting deterministic and available through the embeddable library API without printing or exiting.

Testing: table-driven positive/negative fixtures with expected diagnostic codes, primary spans and optional notes; malformed-input mutation/fuzz tests; regression tests for every reported crash; snapshot tests with deliberate review of wording changes. Keep tests targeted and fast in PR CI. The CLI may render annotated source, but syntax/sema only return structured diagnostics.

Exit: robust recovery on representative malformed programs, stable diagnostic assertions, no crashes in bounded malformed-input fuzzing, and clear differentiation between syntax and semantic failures.

## M2.5 — Cranelift executable backend (implementation committed; CI verification outstanding)

The optional leaf crate `mini-c-cranelift` now consumes one `mini-c-sema::CheckedFunction` at a time, lowering typed expressions, SSA locals, integer conversions, signed/unsigned arithmetic and comparisons, short-circuit conditions, branches, while loops, break/continue, returns and direct calls (including recursion) to Cranelift IR. It emits PIC x86-64 Linux ELF object files that can be linked with `cc`, without introducing Cranelift dependencies into syntax or sema. The standalone `mini-c-cranelift --emit-obj OUTPUT.o INPUT.c` executable and embedded `compile_source` API are available; unsupported pointers, globals and aggregate types return explicit errors. See [Cranelift backend](Cranelift.md).

A dedicated, cached `.github/workflows/cranelift.yml` compiles and tests this crate, builds the generator/backend binaries once and uses `tools/test_cranelift.py` to compare linked Mini-C/GCC executables on arithmetic, control-flow, recursive calls and seeded defined-behavior programs. Heavy Cranelift builds were excluded from the fast syntax workflow's package list. The **exit gate remains a green full backend CI run**; do not mark M2.5 complete before one is recorded. GitHub Actions previously reported failures before runners were assigned (zero steps), so final compilation and runtime verification must be checked separately.

Exit: a reproducible green Cranelift executable smoke suite with GCC comparisons, including loops, branches and calls. The backend remains an optional leaf crate ahead of M3.

## M3 — First executable native vertical slice (initial implementation committed; verification outstanding)

Implemented the first checked C-to-native adapter in [`bingundu/compiler/tools/mini-c-driver`](https://github.com/bingundu/compiler/tree/main/tools/mini-c-driver), in an isolated Cargo workspace. It pins `mini-c-syntax` and `mini-c-sema` to commit `bf1a2b8524b0071ad0439671d3c3965485adeb40`, lowers each checked C function to the **real** `compiler-ir` block-argument SSA, invokes `compiler-opt`, and emits native GNU assembly or direct ELF64 through `compiler-backend-x86`. It uses the same optimizer and native backend as the Oberon compiler, not a second instruction emitter or the independent Cranelift reference backend.

Current signed-i32 subset: parameters, locals with nested scopes/shadowing, arithmetic `+`/`-`/`*`, comparisons and logical expressions, short-circuit `&&`/`||`, `if`/`else`, `while`, `break`/`continue`, and direct calls, including forward prototypes/recursion (up to sixteen SysV integer parameters). A C-to-ELF/assembly/GCC test harness contains fixed cases, seeded bounded arithmetic programs, negative cases and mixed-object interop. Unsupported division/remainder, wide/unsigned arithmetic, pointers, arrays, data objects and void returns are **explicitly rejected** rather than compiled incorrectly.

The adapter builds a separate `compiler-mini-c` executable for development so that the main compiler's private Oberon and fast core Cargo dependency graphs remain unchanged. See [compiler's M3 integration documentation](https://github.com/bingundu/compiler/blob/main/docs/Mini_C_Integration.md). Its separate Actions workflow needs a `MINI_C_DEPLOY_KEY` for the private Mini-C repository. **This code is not yet CI-verified**: first Actions run failed before allocating a runner (zero executed steps). A passing local build/differential run or real CI job remains necessary before claiming the exit gate is met.

Exit: green executable tests of the signed integer/control-flow corpus on x86-64 SysV. Expanding division/remainder and the complete defined-behavior generator corpus should follow without introducing a parallel code generator.

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

### Review checkpoint — incremental parser commits

Reviewed the 16 commits since the incremental architecture contract (including automated rustfmt commits). The streaming source/sink boundary is in place and the targeted syntax tests passed in CI run 39449761594; its Clippy-only failure was fixed in d527087. Treat that CI fix as passing for milestone planning, **not as a verified green run**. M0 is complete. **M1 is not complete**: typedef-aware parser scope updates, general declarators, prototypes, and a usable check CLI remain outstanding. The new recursive-descent expression parser also duplicates the LALRPOP expression grammar; consolidate this before calling M1 finished.

Review findings and follow-ups: the parser currently retains a full input string and only streams AST items; external preprocessing still buffers stdout. Statement nesting and control-flow syntax are supported, but legality of break/continue, lvalue checks, type checks and return compatibility belong to M2. Add explicit tests for typedef shadowing, precedence, malformed expressions, and scope boundaries. The sink contract should eventually update persistent symbol/type state between items. Avoid claiming full streaming input or C conformance until these are addressed.
