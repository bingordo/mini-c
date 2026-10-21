# M2.5 — Cranelift executable reference backend

Mini-C has an **optional, leaf-only** Cranelift lowering crate:
[`crates/cranelift`](../crates/cranelift). This is an executable oracle
independent of the planned `bingundu/compiler` backend, not the production
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
It reads C source directly, **not** through a preprocessor. For macros and
includes, preprocess separately with `cc -E`, supplying the resulting
`.i` file to the CLI. The embedded API
`mini_c_cranelift::compile_source(&str) -> Result<Vec<u8>, String>` emits
the exact same relocatable ELF object bytes in memory. Advanced users can
construct a `mini_c_cranelift::Compiler` and call `add(CheckedItem)`
for one top-level item at a time, followed by `finish()`.

**Supported now:** x86-64 Linux System V calling convention; signed and
unsigned 32-bit `int` and 64-bit `long`; local variables, arguments,
assignments, arithmetic, signed/unsigned division and remainder,
comparisons, unary minus, logical not, &&/|| with correct short-circuit
evaluation; `if`/`else`, `while`, `break`, `continue`, `return`; direct
function calls, forward prototypes, and recursion. Returning from
the end of `main` emits zero. A nonvoid function (other than `main`)
that can syntactically fall through is rejected. Each checked function
is lowered and discarded before the next is accepted.

**Deliberately unsupported:** file-scope data objects and their reads,
pointers/addressing, dereference, arrays, globals, `char` and narrower
integer ABI handling, block-scope `extern` or `static`, aggregates,
functions invoked via function pointers, non-x86-64 platforms and
the full C expression/declarator grammar. Unsupported checked
constructs produce backend errors rather than silently generating code.
The current backend initializes uninitialized locals to zero as an
internal Cranelift SSA convenience; this does not define the behavior
of reading uninitialized variables in C.

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
