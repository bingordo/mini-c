# Cranelift executable reference backend

Mini-C has an **optional, leaf-only** Cranelift lowering crate:
[`crates/cranelift`](../crates/cranelift). This is an executable oracle
independent of the `bingordo/compiler` backend, not the production
code generator. It consumes `mini_c_sema::CheckedFunction`, never the raw
parser AST; neither syntax nor sema depend on Cranelift.

## Build and run

```sh
cargo test -p mini-c-cranelift
cargo build -p mini-c-cranelift -p mini-c-fuzz
cargo run -p mini-c-cranelift -- --emit-obj /tmp/example.o example.c
cc /tmp/example.o -o /tmp/example
/tmp/example

# Compare real linked ELF executables with GCC, with saved reproducers:
python3 tools/test_cranelift.py --seed-count 4 --steps 100
```

`mini-c-cranelift --emit-obj OUTPUT.o INPUT.c` is the standalone CLI.
The standalone CLI uses Mini-C's internal C preprocessor by default and offers
the external `--cpp=cc` fallback for hosted compatibility. The embedded API
`mini_c_cranelift::compile_source(&str) -> Result<Vec<u8>, String>` emits
the exact same relocatable ELF object bytes in memory. Advanced users can
construct a `mini_c_cranelift::Compiler` and call `add(CheckedItem)`
for one top-level item at a time, followed by `finish()`.

## Current support — November 13, 2026

This backend emits x86-64 Linux ELF object files interoperable with GCC under
the System V ABI. It implements integer/pointer arithmetic, local/global
objects, arrays, aggregate layout/field access/copy, control flow
(including nested `switch`, `goto`, loops and recursion), checked function
calls and returns, local/string/global initializers, and the supported C99
declarator and initializer subset. See
[`C99_Frontend_Plan.md`](C99_Frontend_Plan.md) for independent frontend gaps.

The reference backend now supports these additional native ABI slices:

- Scalar `float` (F32) and `double` (F64): constants, local storage,
  unary and binary arithmetic, comparisons, conditional tests, conversions
  and direct/indirect nonvariadic calls and returns using SSE registers.
- `struct`/`union` by-value **register-class** arguments and returns on
  x86-64 SysV when their complete representation occupies at most two
  eightbytes. The classifier supports INTEGER, SSE, and mixed INTEGER/SSE
  classes, including two `float` fields packed into one SSE eightbyte.
- Correct register allocation for the supported aggregate classes with
  independent 6-GP and 8-SSE argument limits; when an aggregate cannot fit,
  code generation rejects it rather than silently splitting it incorrectly.
- Direct GCC executable differentials and GCC/Mini-C mixed-object linking
  in both directions for scalar FP, one/two-register integer aggregates,
  packed-float aggregates, two-double aggregates, and mixed GP/SSE aggregates.

**Still unsupported, fail-closed:** MEMORY-class aggregate arguments/returns
(e.g., larger than 16 bytes, including hidden sret handling), aggregate
register exhaustion that requires stack passing, unaligned aggregate members,
extended/x87 floating-point types (`long double` and `_Float128`),
hexadecimal FP literal code generation, and full SysV variadic calls with
SSE arguments (`%al` vector register count). Floating-point global/static
constant initializers and some C99 language/CPP features also remain work.
The Cranelift backend does not target AArch64. Some missing-return
diagnostics remain stricter than ISO C permits; a zero-initialization
convenience for otherwise-indeterminate automatic locals does not change
the C undefined-behavior rules.

Current validation (source branch
[`cranelift-sysv-aggregate-abi-foundation`](https://github.com/bingordo/mini-c/pull/173)):
[compiler-ci run #43518580978](https://github.com/bingordo/compiler-ci/actions/runs/43518580978)
passed targeted Rust tests, Clippy, preprocessor/hosted-header checks, and
GCC executable and cross-object ABI differentials. The hardcoded branch
override in [compiler-ci PR #669](https://github.com/bingordo/compiler-ci/pull/669)
is temporary validation only and must not be merged.

## C99 switch and goto

The checked frontend handles integer promotions, duplicate/default
diagnostics, fallthrough and nested switch constraints. The Cranelift
backend has CFG-entry blocks for case/default labels inside nested
statements, allows switch dispatches to jump over local declarations,
and preserves ordinary function-scoped goto labels, including jumps
entering a switch. Runtime comparisons against GCC exercise these cases;
switch dispatch currently uses comparator chains rather than dense
jump tables. The final-return gate is based on CFG reachability.

## Independent reference tests

`tools/test_cranelift.py` compiles each fixture with the Mini-C object
emitter and system linker, then separately with `cc -std=c11 -O2`.
It executes both and checks exit status and stdout. The fixtures cover
calls, mutual recursion, loops, shadowing, break/continue, unsigned/long
arithmetic, short circuiting (including divisions that must be skipped),
implicit `main` return, direct libc calls, internal functions, and void calls. It also links mixed object files in both directions (GCC calling a Mini-C-defined function and Mini-C calling a GCC-defined function), checking the x86-64 SysV integer ABI and relocations. The seeded corpus is generated
by the *existing* `mini-c-fuzz` binary without rebuilding it for every
seed. Failing cases preserve input source, command arguments, compiler
version and outputs under `target/cranelift-failures/`; CI uploads the
directory only on failure. Add regressions for any discovered mismatch.

`.github/workflows/cranelift.yml` is a separate cached GitHub Actions
workflow triggered for changes to the backend or its executable test
harness, and by manual dispatch. It tests only this crate, builds
reusable backend/fuzzer binaries once, runs GCC executable differentials,
and runs backend Clippy. The fast syntax CI explicitly excludes the
heavy backend dependencies to minimize CPU time.

Cranelift is pinned to version `0.136.3` in the leaf crate. The crate
uses the host's native ISA and PIC configuration so the emitted object
links via ordinary `cc`. **CI must show a green full backend run**
before M2.5 can be marked verified/complete.
