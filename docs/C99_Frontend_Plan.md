# C99 frontend completion plan (reprioritized 2026-11-09)

**Scope:** `mini-c-syntax`, `mini-c-sema`, and preprocessor front-end interfaces only. Native x86/AArch64, floating-point code generation, ABI, ELF, Cranelift and optimizations are separate projects. The target is *ISO C99 language and constraint semantics*, not GNU C or C23. Hosted library completeness is explicitly separate. Do not block valid checked C99 ASTs on backend capabilities.

## Future real-program testing — Csmith (after core C99/CPP gates)

**Tracked by [#116](https://github.com/bingordo/mini-c/issues/116)** with the already measured frontend blocker in [#99](https://github.com/bingordo/mini-c/issues/99). Plan this as a **later end-to-end acceptance milestone**, not as immediate F5 work. Initial reference evidence from eight original Csmith 2.3.0 restricted-observable programs showed GCC/Clang 8/8 agreement and Mini-C native acceptance 0/8 at the pinned historical revision. Do not replace those unchanged original programs or report parse-only success as native success.

1. Preserve the source-pinned eight-file corpus, generator version, compiler options and checksums; maintain independent GCC/Clang reference and sanitizer or defined-behavior checks.
2. Audit external and eventually internal CPP against original Csmith sources and document token/source-map differences distinctly from parser or checker failures.
3. Check unmodified source through Mini-C syntax and semantic IR, track first real diagnostics; resolve practical C99 gaps in F1–F4 and any backend FP/ABI gaps in their owning projects.
4. Compile, link, **execute** and compare all eight native programs against GCC/Clang; report exact denominators and output/exit status. Expand to reproducible seeded Csmith CI only after this fixed baseline passes.
5. Keep extended seeded runs in `bingordo/compiler-ci` with failure artifacts, versions, reproducible seeds and minimization. Separate errors by CPP, frontend, IR, backend, runtime and reference disagreement.

This milestone should not prevent progressing to a usable C compiler and C++ support; defer F5 until separately resumed, unless a specific real-program blocker is deliberately reprioritized.

## Active priorities and milestone ownership (2026-11-09)

**Priority change:** Keep F1 (core statements) and practical F2/F3/F4 C99 frontend slices active. **Defer F5** (advanced numeric/lexical C language semantics) until practical C support and the internal C99 preprocessor are usable. Deferred does **not** mean completed, removed, or optional for a future full-C99-conformance claim.

**CPP-C99: practical internal preprocessing now defaults on, remaining conformance deferred**, tracked by [#110](https://github.com/bingordo/mini-c/issues/110) and child issues [#111](https://github.com/bingordo/mini-c/issues/111)–[#114](https://github.com/bingordo/mini-c/issues/114). Work in parallel with F1/F2. External `cc -E` preprocessing and origin mapping already passed historical M6; this work specifically graduates the feature-gated self-contained `--cpp=internal` from experimental to production-eligible for ISO C99. C23 CPP extensions are not a release gate.

**Separation of responsibility:** The CPP milestone owns preprocessing translation phases, directive parsing, `_Pragma`, macro expansion, include search, predefines, source origins and resource limits. F5 retains wide character/string and numeric *language* semantics plus post-preprocessing lexical constraints. CPP must not block on native code generation or the deferred F5 language work. Do not call the entire C99 frontend conformant until F5 and final F6 validation eventually close.

**Near-term order:** finish F1 switch and _Bool slices; concurrently CPP-C99/1 macro/phase parity and CPP-C99/2 include/predefine support; then CPP-C99/3 provenance and CPP-C99/4 resource/CI graduation. Continue practical F2/F3/F4 increments driven by real-source acceptance. Leave F5 deferred and F6 final language conformance last.

## Immediate CPP-C99 focus — usable programs (2026-11-10)

The priority is **a working self-contained C99 preprocessor for practical programs**, not another round of resource-budget hardening. Retain the already merged input/macro/output safety guards, but put new CPP/4 memory/CPU instrumentation and tuning on the follow-up queue until real-program compatibility is stronger.

1. **CPP/2 first:** production hosted header search, target predefines, `-isystem`/`--sysroot` correctness, and unmodified glibc/musl header processing. [Mini-C PR #129](https://github.com/bingordo/mini-c/pull/129) enabled system-header discovery and proved unmodified `<stdio.h>` through native `printf` Hello World in [compiler-ci #42933767715](https://github.com/bingordo/compiler-ci/actions/runs/42933767715). [compiler-ci PR #336](https://github.com/bingordo/compiler-ci/pull/336) makes it a scheduled acceptance gate.
2. **CPP/1 and CPP/3 as blockers are found:** fix macro/directive conformance and actionable source mapping when actual programs expose gaps, rather than exhaustively auditing low-value variants ahead of integration. Strict GCC/Clang differential tests remain.
3. Add unmodified real-source smoke cases to `bingordo/compiler-ci`: [musl `strcmp.c`](https://github.com/bingordo/mini-c/blob/main/tests/fixtures/musl-s10/string/strcmp.c) passed checker + native object with internal CPP in [compiler-ci #42934545452](https://github.com/bingordo/compiler-ci/actions/runs/42934545452), and is tracked for permanent CI in [compiler-ci PR #337](https://github.com/bingordo/compiler-ci/pull/337).
4. **CPP/4 later:** complete peak-RSS, allocation and stress benchmarks after core hosted C coverage, keeping the existing guardrails. The internal option is now the driver default for practical C work; broader ISO C99 CPP conformance still has independent outstanding gates.

Csmith original-program native execution remains scheduled under [#116](https://github.com/bingordo/mini-c/issues/116). **F5 remains deferred**; do not delay a usable C compiler or initial C++ support for exhaustive C99 conformance.

## Ground truth and validation rules

This plan supersedes stale feature-gap sections in `docs/Semantics.md`, `docs/Specification.md` and `docs/Next_Gaps.md`; those files still describe parts of the already implemented enums, aggregates, decimal/hex floating tokens, arithmetic conversions, variadic declarations and nested recovery as missing. Validate each milestone against **current source**, not milestone completion claims.

For every feature, require:
1. Positive parsing and checked-IR tests, including type, value category, implicit conversions, layout, lifetime/scope, and evaluation-order representation.
2. Negative **constraint** tests with stable diagnostic categories and source locations. Reject unsupported features explicitly; never silently erase qualifiers or extensions with semantic effect.
3. A deterministic feature matrix with independent GCC **and** Clang references using `-std=c99 -pedantic-errors -fsyntax-only`. Distinguish required diagnostics, implementation-defined behavior, and outright forbidden constructs; do not equate every GCC extension/warning with a normative C99 rule.
4. A generated family of valid and invalid test programs, reproducible seeds, shrunk/minimized failures, and parser-vs-checked-IR differential tests. Preserve exact inputs and toolchain versions.
5. Focused `mini-c-syntax` / `mini-c-sema` tests and Clippy on each PR, wider checks at milestone boundaries. **No native backend pass required.**

Keep the incremental item-at-a-time checker and the existing external `cc -E` preprocessing path; the experimental internal preprocessor has its own separate conformance gate. Do not use fake “accepted” Csmith statistics when the backend cannot execute the program.

## F0 — Audit and test matrix

- [x] Record a source-based baseline of missing constructs (this file).
- [x] Add `tools/c99_features.json`: machine-readable positive/constraint rows with feature IDs, current Mini-C support/gap status and source test references.
- [x] Add `tools/c99_frontend_matrix.py`: strict independent GCC/Clang C99 syntax+constraint acceptance oracle, Mini-C comparison, preserved reference verdicts and explicit coverage gaps. Compile-time C99 `sizeof` type witnesses and `c99_f0_feature_matrix_checked_shapes_are_not_only_exit_codes` assert checked-IR types/kinds; a future larger corpus must extend these assertions feature by feature.
- [x] Generate bounded reproducible variants from explicit valid/invalid grammar fixtures. Record seed, generated source SHA, compiler commands, tool versions, acceptance, exact first diagnostic, and artifacts for unexpected discrepancies; optionally apply a bounded source-line reducer without changing the independent reference verdict. CI uploads the full coverage report.

### F0 acceptance and usage

Current baseline (2026-11-08): **22 cases × 2 deterministic variants = 44 source-only probes**; strict GCC and Clang agree with the normative fixture labels, Mini-C matches 22 probes and explicitly does not accept 22 *valid C99* gap probes. There were zero unexpected mismatches in [syntax CI run #42499920995](https://github.com/bingordo/mini-c/actions/runs/42499920995). These gaps **do not count** toward conformance. Focused Rust checked-IR assertions verify types and expression shape; additional features must expand those assertions.

Run after `cargo build -p mini-c`:

```sh
python3 -m unittest discover -s tools/tests -p 'test_c99_frontend_matrix.py'
python3 tools/c99_frontend_matrix.py \
  --mini target/debug/mini-c --seed 99 --variants 2 --minimize \
  --report target/c99-f0/report.json --artifacts target/c99-f0/failures
```

The CI syntax job runs this independently of native backend/FP execution. Every unexpected mismatch fails CI and preserves source, SHA, reference commands, compiler diagnostics and optionally a minimized repro. The suite is a **baseline**, not a complete C99 conformance claim. New F1–F5 features should convert recorded `mini: gap` rows to `mini: supported` only after independent semantic tests and the GCC/Clang matrix pass.

## F1 — Core statements, objects and specifiers (highest priority)

- [ ] `switch` with integral promotions, duplicate case/default detection, constant-expression validation, nested switches, case fallthrough and switch-aware break.
- [ ] Ordinary identifiers as labels, `goto`, forward references, duplicate/missing label diagnosis and function scope; preserve explicit control flow in checked IR.
- [ ] `_Bool` distinct from unsigned char, including 0/1 conversion, promotions, `sizeof`, parameters/returns and aggregate layout.
- [ ] `volatile` qualifier preservation/compatibility, qualified lvalue accesses and compound operations. No loss of observable access semantics.
- [ ] `inline` function specifier and C99 external-definition / linkage compatibility (not optimization inlining).
- [ ] `__func__` predefined per-function string object with correct scope and array semantics.

## F2 — Expressions and conversions (start immediately; independent slices)

- [x] **F2.1:** `sizeof unary-expression`, including `sizeof x` and `sizeof(x)`, no operand evaluation or array decay, incomplete/function/void rejection, and constant-bounded abstract arrays. Implemented in PR #105, verified by syntax/semantics plus Cranelift/glibc compatibility CI. Runtime VLA `sizeof` remains F4.
- [ ] General arithmetic casts, including integer↔float, float↔float, scalar→`_Bool`; preserve target conversions in checked IR.
- [ ] C99 pointer subtraction, relational comparison constraints, conditional expression types, null pointer constant subtleties and qualified composite pointer types.
- [ ] Complete usual arithmetic conversions, default promotions at varargs/non-prototype calls and complex types, without requiring executable codegen.
- [ ] `sizeof`/alignment and compile-time constant-expression rules for every scalar/aggregate category.

## F3 — Initializers and aggregates

- [ ] Nested designated initializers (`[index]`, `.member` and mixed chains), overlapping designator semantics and array-bound inference.
- [ ] Compound literals with correct lvalue and storage-duration semantics, including nested aggregate forms.
- [ ] Struct/union bit-field widths/layout, zero-width fields, unnamed fields and address-of constraints; compare target layout to GCC/Clang.
- [ ] Flexible array members and restrictions, flexible aggregate size/initialization.
- [ ] Complete aggregate copy, string-array initialization, padding, linkage/static initializer constant-address expressions.

## F4 — Declarators, storage duration and compatibility

- [ ] Variable-length arrays and variably modified types, block/parameter scope rules, array parameter syntax and VLA `sizeof` evaluation.
- [ ] C99 old-style function declaration/definition compatibility and default argument promotions; prohibit invalid redeclarations.
- [ ] Complete type compatibility/composite types across translation units; tentative definitions, storage/linkage and C99 inline definitions.
- [ ] Qualifier combinations and constraints (`const`, `volatile`, `restrict`) across pointers, typedefs, function parameters and lvalues.

## F5 — Remaining normative C99 numeric/lexical rules (DEFERRED)

Do not schedule new F5-specific work now. Preserve all acceptance criteria for later; a full C99 frontend conformance claim will require finishing them. CPP-owned C99 translation phases, `_Pragma` expansion and preprocessing tokenization have been promoted to the active CPP-C99 milestone below. C23 numeric/lexical additions are not currently in scope.


- [ ] `_Complex` and `_Imaginary` types and usual arithmetic semantics; distinguish conditional C99 support from required core.
- [ ] Real and complex floating conversions, hex/decimal literal target rounding, signed zero, floating constant-expression constraints and floating environment implications.
- [ ] Wide character/string literals, universal character names, execution/source character-set rules, adjacent string literal concatenation and lexical translation phases.
- [ ] Complete C99 grammar constraints, keywords and diagnostics (including `_Pragma` preprocessing syntax).

## CPP-C99 — Complete the internal preprocessor (ACTIVE P0)

**Tracker:** [#110](https://github.com/bingordo/mini-c/issues/110). The verified M6 external path remains available via explicit `--cpp=cc`. The internal `rcc_preprocess` adapter is now enabled by default in the two Mini-C binary crates; the core frontend remains independent of it. This is a pragmatic driver-default decision, not closure of every production-readiness/conformance gate. This is completion work, not a replacement of Mini-C's parser/sema with another C frontend.

- [ ] **CPP/1: C99 phase, directive and macro parity** ([#111](https://github.com/bingordo/mini-c/issues/111)). Cover C99 translation phases, trigraphs/digraphs, line splicing, pp-token formation, comment removal, object/function/variadic macros, argument expansion, `#` and `##`, rescan/hide sets, conditionals, `#line`, `#pragma`, `_Pragma`, and malformed directive diagnostics.
- [ ] **CPP/2: target environment and include resolution** ([#112](https://github.com/bingordo/mini-c/issues/112)). Explicit supported x86-64 LP64 predefined macros, quoted/angle include lookup, reproducible include roots and defines, macro-generated `#include`, nested headers, path/error handling, and representative real libc headers. Do not silently claim GCC-only extensions.
- [ ] **CPP/3: source/macro provenance** ([#113](https://github.com/bingordo/mini-c/issues/113)). Track spelling vs invocation vs argument origin and nested expansion/include locations through `#`/`##`; produce useful source-mapped frontend diagnostics without fabricated precise columns.
- [ ] **CPP/4: robustness and production graduation** ([#114](https://github.com/bingordo/mini-c/issues/114)). Enforce bounded expansion/token/byte/include budgets *inside* the engine (not post-allocation), deterministic adversarial/fuzz regressions, bounded CI plus 128+ seeded extended checks, GCC/Clang comparisons, minimizable artifacts, check/Cranelift integration, and measured CPU/memory/startup overhead. Document remaining deviations and continue the unresolved robust/conformance checks without blocking practical driver usage.

**Exit:** Independent C99 preprocessing token/directed-error differential acceptance against GCC and Clang on a pinned conformance corpus, reproducible reference versions/seeds, stable source mapping, bounded resource use, representative real-header acceptance and passing CI for both Mini-C drivers. Keep external `--cpp=cc`/`--cpp=clang` and `--cpp=none` available. The internal driver default is enabled for practical work, but **do not** claim complete ISO C99 conformance while tests and features are missing. C23 `__VA_OPT__`, `#elifdef`, `#embed` etc. are follow-ups, not C99 completion gates.

## F6 — Final C99 conformance matrix (AFTER F5; CPP work tracked separately)

- [ ] Treat verified external `cc -E` and completed CPP-C99 as separate preprocessing paths; verify both through the final language conformance matrix, keeping the external fallback available.
- [ ] Expand automated feature-matrix differential across GCC/Clang versions and x86-64 LP64 target; collect implementation-defined assumptions, track independent expected types.
- [ ] Exhaustive negative syntax/constraint tests, malformed-input fuzzing and stable error spans.
- [ ] Produce an explicit C99 support statement and known deviations; **only** claim full frontend C99 when every required row is green.

## Execution order and first slice

**Updated:** F2.1 is complete. Continue F1 switch and `_Bool`, practical F2/F3/F4 work, and the four CPP-C99 slices in parallel. Defer F5. F6 is the eventual final frontend conformance gate after F5 is resumed. Never conflate typing FP expressions with native FP codegen, and do not call the internal CPP production-ready before the #110 exit gate passes.
