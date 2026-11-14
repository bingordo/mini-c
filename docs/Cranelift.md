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

The reference backend now supports these native ABI slices:

- Scalar `float` (F32) and `double` (F64): decimal constants, local
  storage, arithmetic, comparisons, conditional tests, conversions, and
  direct/indirect nonvariadic calls and returns using SSE registers.
- `struct`/`union` by-value arguments and results in the x86-64 SysV
  **INTEGER, SSE, and mixed INTEGER/SSE register classes** when the
  complete representation fits in up to two eightbytes.
- SysV **MEMORY-class aggregate parameters** (including a small aggregate
  forced to the stack by GP/SSE register exhaustion), using Cranelift's
  `StructArgument` purpose. Outgoing stack copies are eightbyte-padded
  without reading beyond a smaller original C object.
- SysV **MEMORY-class aggregate returns** using the caller-provided hidden
  `StructReturn` address, including functions with both stack-passed
  aggregates and additional integer/FP parameters.
- **Direct scalar variadic calls**, including GP and F32/F64 arguments.
  C default promotions convert `float` ellipsis arguments to `double`.
  A callsite-specific tail thunk sets `%al` to the count of occupied XMM
  argument registers (0–8), including fixed FP parameters, without
  disturbing argument registers or stack areas.
- GCC executable and mixed GCC/Mini-C object differentials in both
  directions for register/memory aggregate passing and returning,
  20-byte stack objects, scalar floating-point operations, and direct
  variadic calls into GCC functions using `va_arg(double)`.

**Bounded implementation:** The current MEMORY-class path copies
aggregates of at most **4,096 bytes** with straightforward inline copy
code. Larger by-value aggregates fail explicitly until shared memcpy
lowering is implemented. This is an implementation limit, not a C or
SysV ABI limit.

**Still unsupported, fail-closed:** indirect variadic function calls,
aggregate types passed through `...`, extended/x87 floating-point types
(`long double` and `_Float128`), hexadecimal FP literal code
generation, some floating-point global/static constant initializers,
and unrelated C99 language/CPP gaps. The Cranelift reference backend
does not target AArch64. The current register classifier covers the
implemented INTEGER/SSE cases; additional exotic aggregate ABI classes
and unsupported target layouts need further validation. The backend
uses deterministic initialization for otherwise-indeterminate local
scalars as an implementation convenience, not as a C semantic promise.

Validated on `bingordo/compiler-ci`, not `bingordo/compiler`:

- [Run #43530563535](https://github.com/bingordo/compiler-ci/actions/runs/43530563535):
  MEMORY-class struct arguments, hidden returns, GP exhaustion and
  20-byte aggregate copies — targeted Rust tests, Clippy and executable
  GCC/cross-object tests passed
- [Run #43531054795](https://github.com/bingordo/compiler-ci/actions/runs/43531054795):
  direct floating-point variadics, GCC `va_arg(double)`, more than eight
  FP arguments and all existing regressions — corresponding gates passed

Validation source routing uses temporary
[compiler-ci PR #669](https://github.com/bingordo/compiler-ci/pull/669).
Its hardcoded Mini-C test branch must **not** be merged.

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

The active validation workflow is
[`bingordo/compiler-ci/.github/workflows/mini-c-cranelift.yml`](https://github.com/bingordo/compiler-ci/blob/main/.github/workflows/mini-c-cranelift.yml),
which builds the backend/fuzzer once and runs the Rust regression suite,
Clippy, hosted preprocessing checks and GCC executable differentials.
Do not use `bingordo/compiler` Actions while its minutes are exhausted.

The leaf crate depends on Cranelift `0.136` and emits native x86-64
PIC ELF objects linkable with `cc`. This is an executable reference
backend, not the eventual `bingordo/compiler` native backend.
