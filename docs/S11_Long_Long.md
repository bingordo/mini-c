# S11 — distinct `long long` rank and LL/ULL suffixes

**Status:** implemented and merged 2026-11-03.

## Representation and semantics

C syntax accepts two `long` type specifiers in the supported declaration
orders, with optional `signed`, `unsigned`, and `int`. A third `long`
remains invalid. `long long` and `long` have **distinct integer ranks**,
including on LP64 where both occupy 64 bits.

The typed syntax scanner decodes decimal, hexadecimal and octal constants
with `LL`, `LLU`, `ULL`, and their case-insensitive equivalents.
Invalid combinations and third `L` are rejected rather than split
into multiple tokens.

The semantic first-representable-type selection now includes
`long long` and `unsigned long long`. Key differences:

- Unsuffixed decimal literals choose signed int, long, then long long.
  A value exceeding the signed long long range is rejected.
- Unsuffixed hexadecimal/octal literals can choose unsigned candidates,
  including unsigned long long.
- Decimal `LL` may select only signed long long, while hexadecimal/octal
  `LL` may fall back to unsigned long long.
- `ULL` always denotes unsigned long long within the supported u64 range.
- Integer rank remains distinct in usual arithmetic conversions even if
  two types happen to use the same number of bits.

`Target` retains its existing `int_bits` and `long_bits` fields;
`long long` has a fixed **64-bit** representation in this subset,
so the model supports an ILP32-style 32-bit long with 64-bit long long
as well as LP64. Values beyond u64 and extended integer types are
not modeled.

## Validation

[Mini-C PR #45](https://github.com/bingordo/mini-c/pull/45) merged
after syntax, generated parser, and Cranelift executable tests passed.
Tests cover C specifier orders, invalid third `long` and malformed
suffixes, ILP32 first-fitting type selection, signed overflow
rejection, unsigned fallback, and GCC executable comparison.

[Compiler PR #299](https://github.com/bingordo/compiler/pull/299)
synchronizes both compiler frontend pins and maps `LongLong` to
the native 64-bit x86 SSA and SysV ABI. GCC differential tests check
signed/unsigned calls, 64-bit arithmetic, pointer mutation, local
arrays and mixed-rank comparisons. Both the main compiler CLI and
standalone native driver integration workflows passed before merge.

## Deliberate boundaries

This is support for a 64-bit `long long` subset, not a complete C
integer implementation. Further work includes platform-specific
integer data models beyond the modeled widths; extended integer and
floating-point types; broader ABI differential testing across data
structures and variadic calls; and further coverage of real-world
Csmith/musl inputs. The existing S9 pinned musl gate remains intact.
