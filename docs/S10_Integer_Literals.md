# S10 — C integer constants (2026-11-03)

## Implemented

The scanner recognizes decimal, hexadecimal (`0x`/`0X`) and octal
constants with optional `U`, `L`, `UL`, or `LU` suffixes in either
letter case. A shared decoder rejects malformed digits, repeated suffix
letters, unsupported `LL` and values beyond `u64`.

The syntax AST preserves **radix and suffix** whenever those affect
type selection, without breaking the historical unsuffixed decimal
`Expr::Int` API. Semantic analysis selects the *first representable
type* using the target's `int_bits` and `long_bits`, matching the
C17 integer-constant rules within the supported types:

| Spelling | Candidate types (in selection order) |
| --- | --- |
| decimal, no suffix | `int`, `long` |
| hex/octal, no suffix | `int`, `unsigned int`, `long`, `unsigned long` |
| decimal or hex/octal, `U` | `unsigned int`, `unsigned long` |
| decimal, `L` | `long` |
| hex/octal, `L` | `long`, `unsigned long` |
| any base, `UL` / `LU` | `unsigned long` |

This includes the LP64 cases `0xffffffff` as `unsigned int`,
`0x100000000` as signed `long`, and `0xffffffffffffffff`
as `unsigned long`. The choices differ on a target with 16-bit
`int`; these cases have dedicated semantic tests.

## Validation

- [Mini-C PR #39](https://github.com/bingordo/mini-c/pull/39)
  merged the common decoder, parser/AST and semantic changes after
  [syntax CI](https://github.com/bingordo/mini-c/actions/runs/41701399358),
  [generated-parser CI](https://github.com/bingordo/mini-c/actions/runs/41701399683)
  and [Cranelift/GCC CI](https://github.com/bingordo/mini-c/actions/runs/41701399291)
  passed.
- [Compiler PR #284](https://github.com/bingordo/compiler/pull/284)
  synchronizes both independent frontend pins and adds native GCC
  differential coverage for hex/octal, `U`/`L`, and the full 64-bit
  unsigned range. Both normal compiler integration checks passed.

The original S9 musl acceptance gate remains 12/12. These tests
establish behavior for specific inputs and models, **not** complete
C17 or C23 conformance.

## Remaining gaps

1. Add an explicit `long long` rank if broader C compatibility requires
   `LL` and `ULL` suffixes; retaining only `int` and `long` is
   insufficient on targets where `long` is 32-bit.
2. Add hexadecimal floating constants and floating-point scalar types
   separately rather than treating them as integer tokens.
3. Extend real-world/GCC differential corpora with wider constant
   expressions, target-width changes, and signed/unsigned conversion
   boundary cases before relaxing any rejection rules.
