# Experimental internal C preprocessor for Mini-C

**Status:** optional implementation prototype, not yet a conforming or default
preprocessor. Mini-C's standalone lexer, parser, semantic checker and
Cranelift backend remain unchanged and independent of other compiler
frontends.

## Design

`crates/preprocess` has an optional `internal-rcc` feature using the
`rcc_preprocess` C99 preprocessing engine from
[noname0310/rcc](https://github.com/noname0310/rcc), pinned to revision
`aa7ba4115a986cd533965ae297de6490315c35cb`. This adds a compile-time
dependency only when requested. It **does not** embed the rcc parser, semantic
analysis, IR or backend. Neither `mini-c-syntax` nor `mini-c-sema` depends
on rcc.

The internal adapter converts its expanded preprocessing tokens to
unambiguously token-separated C text, retaining source-file/line markers for
non-synthetic token origins where available. Synthetic macro-token origins
are currently incomplete. The existing Mini-C lexer and parser consume the
result normally. No external preprocessor or shell is invoked.

The external `cc -E` backend is still the default and stays available as
the oracle for differential tests; `--cpp=none` is unchanged. To try the
experimental internal mode:

```sh
cargo run -p mini-c --features internal-cpp -- check \
  --cpp=internal -I include -DFEATURE=1 file.c

cargo run -p mini-c-cranelift --features internal-cpp -- \
  --emit-obj /tmp/file.o --cpp=internal -I include file.c
```

Only `-I`, `-D`, `-U` and the implicit `-E` driver argument are
mapped into the C99 engine; unsupported CPP switches fail with a clear
error, rather than being silently ignored. OS arguments are passed directly
to the internal library without shell interpretation.

## Conformance gate

In `compiler-ci`, the feature tests exercise C macro expansion and the
standalone checker/object emitter. The separate
`crates/preprocess/tests/internal_differential.rs` compares *token
sequences* with GCC on:

- object and function macros, nested calls and rescan/hide sets;
- stringification, `##` pasting with raw arguments, indirect pasting;
- variadic functions and multiline macro invocations;
- `#if`/`defined`, guarded nested includes and `-I/-D/-U`;
- included-header source location markers and expected directive errors.

This is the initial, deliberately small corpus. It neither proves full C99
conformance nor covers C23. Tests compare tokens, never whitespace or
preprocessor-specific directives.

## Conformance-hardening checkpoint (2026-11-01)

The optional adapter now has GCC **and** Clang token-level comparisons
for nested/function/empty/variadic macros, raw vs indirect token pasting,
stringification, recursion hide sets, #if arithmetic, multiline splicing,
comments, guarded and unguarded includes, #pragma once, direct #line and
__LINE__ behavior, and command-line -D/-U redefinitions.

Mini-C normalizes ordered -D/-U operations per macro before constructing
rcc's command-line options. The pinned rcc engine internally applies all
defines before all undefines, unlike GCC/Clang's argument order; the adapter
now preserves the effective final operation for each macro. For duplicate
-DNAME definitions GCC may emit warnings that the adapter does not
reproduce; the expanded token result is compared.

The adapter also consumes rcc's **logical** #line overrides for both its
emitted source markers and preprocessor errors, including virtual file names
and header paths. Native Mini-C frontend diagnostics continue to use the
existing line-marker mapping.

**Known upstream defect — [Mini-C issue #12](https://github.com/bingordo/mini-c/issues/12):**
The pinned expander loses macro invocation location for dynamic __LINE__ /
__FILE__ tokens in replacement lists. A GCC test gives 103 for
`#define WHICH_LINE __LINE__` invoked at logical line 103, but rcc gives
102 (definition line). Rather than silently miscompile a supported C
program, the adapter conservatively rejects affected dynamic-builtin
expansions and points the caller to `--cpp=cc`. The exact GCC reproducer
remains in CI marked ignored until an upstream fix propagates invocation
spans through substitution/rescanning. This guard can reject some valid
cases and is not yet a substitute for complete macro-origin tracking.

Resource limitations remain: the 16 MiB output guard is applied after rcc
returns its token vector, not during internal expansion. Full bounded-time
and bounded-allocation preprocessing needs upstream resource-budget support.
No C23 conformance claim is made.

## Before enabling by default

1. Increase the GCC/Clang differential corpus substantially and minimize
   mismatches reproducibly. Test arbitrary macro interactions, trigraphs,
   comments, phase ordering, `_Pragma`, `#line` and target-specific built-ins.
2. Preserve complete expansion-origin mapping (including synthetic
   `#`/`##` token origins) and render diagnostics against original
   macro invocations and included headers.
3. Add differential testing for C23 additions such as `__VA_OPT__`,
   `#elifdef`, `#elifndef`, `#embed` and `__has_embed`.
   The pinned rcc candidate implements C99; some newer facilities may
   require extending the engine or reconsidering the dependency.
4. Validate target predefined macros and system-header search roots for
   supported host/target triples. Passing the built-in macro environment
   of GCC as though we fully implement its extensions would be misleading.
5. Test resource budgets and error recovery on adversarial include graphs,
   deeply nested macros and large generated sources; compare startup and
   compilation CPU time against the existing external preprocessor.
6. Decide whether to vendor/refactor the narrow preprocessing core after
   benchmarking API stability and dependency costs. Keep Mini-C's core
   crates and checked IR backend-neutral either way.

This deliberately does **not** switch Mini-C to the `cinrs-core` frontend or
its parser, semantic representation or Rust code generator. Mini-C remains a
separate compiler frontend.
