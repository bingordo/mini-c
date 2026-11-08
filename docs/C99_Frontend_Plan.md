# C99 frontend completion plan (2026-11-08)

**Scope:** `mini-c-syntax`, `mini-c-sema`, and preprocessor front-end interfaces only. Native x86/AArch64, floating-point code generation, ABI, ELF, Cranelift and optimizations are separate projects. The target is *ISO C99 language and constraint semantics*, not GNU C or C23. Hosted library completeness is explicitly separate. Do not block valid checked C99 ASTs on backend capabilities.

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
- [ ] Add machine-readable C99 feature rows with references to parser/sema tests and required diagnostics.
- [ ] Add GCC/Clang acceptance harness for source-only checks. Compare structured checked types, not just “both exited 0”.
- [ ] Generate bounded positive/negative examples by grammar feature; save failed seeds and exact first diagnostic.

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

## F5 — Remaining normative C99 numeric/lexical rules

- [ ] `_Complex` and `_Imaginary` types and usual arithmetic semantics; distinguish conditional C99 support from required core.
- [ ] Real and complex floating conversions, hex/decimal literal target rounding, signed zero, floating constant-expression constraints and floating environment implications.
- [ ] Wide character/string literals, universal character names, execution/source character-set rules, adjacent string literal concatenation and lexical translation phases.
- [ ] Complete C99 grammar constraints, keywords and diagnostics (including `_Pragma` preprocessing syntax).

## F6 — Preprocessing and final conformance matrix

- [ ] External `cc -E` remains the default working preprocessing path. Conformance of the internal optional Rust CPP is a separate gate: translation phases, macro rescan/`#`/`##`, variadics, trigraphs, `_Pragma`, `#line`, include search and predefined macros.
- [ ] Expand automated feature-matrix differential across GCC/Clang versions and x86-64 LP64 target; collect implementation-defined assumptions, track independent expected types.
- [ ] Exhaustive negative syntax/constraint tests, malformed-input fuzzing and stable error spans.
- [ ] Produce an explicit C99 support statement and known deviations; **only** claim full frontend C99 when every required row is green.

## Execution order and first slice

**Start F2.1 `sizeof(expression)` immediately** because it is small, source-independent, and exposes the required distinction between an unevaluated operand's type and its ordinary rvalue conversion. Then start F1 switch and `_Bool` in separate PRs. Never conflate the fact that a frontend *types* floating expressions with native FP codegen. Track checks and revisions in the PR description, and keep incomplete work unmerged.
