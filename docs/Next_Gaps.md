# Mini-C next implementation slices (2026-10-29)

This is an actionable gap queue, not a claim that these features have been implemented. Cilly integration is deferred.

## Verified by source inspection
- `crates/cranelift/src/lib.rs` supports local stack-backed scalars/arrays, address-of, dereference and pointer indexing; `Compiler::add` explicitly rejects global objects.
- `crates/sema/src/lib.rs` has array decay and typed lvalues, but `CheckedExpr` has no expression-specific span.
- `docs/Cranelift.md` documents no function-pointer invocation, global objects, or aggregates.

## Recommended implementation order

### A. Global objects and string literals
1. Add typed global definitions/declarations and storage duration rules without conflating tentative definitions, `extern`, and `static`.
2. Emit read-only string storage and relocatable addresses; implement array initialization with explicit bounds and trailing zero rules.
3. Emit writable globals and relocations in the Cranelift oracle; keep this leaf-only.
4. Add mixed-object GCC/Mini-C tests: external data, internal linkage, pointer to global, string pointer, multiple translation units, tentative definitions and duplicate definitions.
5. Gate: run linked binaries and compare outputs/exit status with GCC/Clang; reject unsupported relocations explicitly.

### B. C scalar completeness
- Finish `char`, short, integer promotions, casts, shifts, bitwise operators, conditional expressions, increment/decrement and compound assignments as needed.
- Verify integer-width, signedness and overflow-sensitive semantics through defined-behavior corpus tests.
- Test expression evaluation order without depending on unspecified ordering.

### C. Aggregates and calls
- Add struct/union/enum layout and member access before broad aggregate ABI support.
- Add function-pointer calls and test ABI classifications against GCC.
- Explicitly reject unsupported variadic or aggregate calls until implemented.

### D. Native shared-backend parity
- Recheck the `compiler` repository's Mini-C adapter pin and native M3 CI status.
- Extend shared IR memory, globals, 64-bit/unsigned operations, and relocations in vertical slices.
- Do not duplicate native instruction selection or register allocation in Mini-C.

### E. Diagnostics and scale
- Add expression-level spans and nested-statement recovery.
- Stream preprocessor output and avoid retaining the whole translation unit.
- Extend mutation, differential and ABI tests; run focused CI on each change, broad CI only at milestone boundaries.

## Immediate coding target

**A1: global declarations and object emission**, preceded by tests for `int g=7; int main(void){return g;}`, `static int g;`, `extern int g;`, and multi-file references. Inspect semantic definition tracking and backend symbol APIs first. Implement in small verified vertical slices rather than changing the parser, sema and backend speculatively in one patch.
