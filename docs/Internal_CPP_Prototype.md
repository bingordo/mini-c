# Experimental internal C preprocessor for Mini-C

**Status:** optional implementation prototype, not yet a conforming or default
preprocessor. Mini-C's standalone lexer, parser, semantic checker and
Cranelift backend remain unchanged and independent of other compiler
frontends.

## Active completion milestone — CPP-C99 (P0)

The previously optional experimental path is now **scheduled for completion**, tracked by [#110](https://github.com/bingordo/mini-c/issues/110), in parallel with F1/F2. The implementation **remains opt-in** until the production exit gate is met. Current completed M6 external `cc -E` preprocessing stays the default and supported fallback.

- [#111](https://github.com/bingordo/mini-c/issues/111): audit/fix C99 preprocessing phases, macro rescan/expansion, directives, `_Pragma` and negative constraints against GCC+Clang.
- [#112](https://github.com/bingordo/mini-c/issues/112): target/host predefines, quote/angle include search, header roots and representative real headers.
- [#113](https://github.com/bingordo/mini-c/issues/113): maintain separate spelling/argument/invocation provenance through nested macros and headers and expose honest mapped diagnostics.
- [#114](https://github.com/bingordo/mini-c/issues/114): enforce expansion/token/include budgets inside the engine, regression fuzzing, performance and CI graduation checks.

**C99 scope only.** C23 extensions and F5 advanced lexical/numeric language work are deferred. Full ISO C99 preprocessing support must be demonstrated using token and diagnostic differentials plus adversarial negative cases, not inferred from compiling a handful of fixtures. Do not enable `--cpp=internal` as default or mark complete without these gates.

## Design

`crates/preprocess` has an optional `internal-rcc` feature using a
**narrow local patch** of the `rcc_preprocess` C99 preprocessing engine from
[noname0310/rcc](https://github.com/noname0310/rcc), pinned to revision
`aa7ba4115a986cd533965ae297de6490315c35cb`. Only the preprocessor
crate is copied into `vendor/rcc_preprocess`; its other runtime dependencies
continue to come from the pinned upstream Git revision. The original
`rcc_preprocess` Cargo metadata is MIT OR Apache-2.0. This adds a
compile-time dependency only when requested. It **does not** embed the rcc parser, semantic
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

**Fixed invocation-source defect — [Mini-C issue #12](https://github.com/bingordo/mini-c/issues/12):**
The original rcc expander lost macro invocation locations for dynamic
`__LINE__` and `__FILE__` tokens in replacement lists. The isolated patch
adds a separate `expansion_site` to each expansion token, propagating it
through object/function macro substitution, indirect expansion and token
pasting. Token *spelling* spans are unchanged for correct `#`/`##` behavior.
The builtins now use the invocation-site span, including `#line`
adjustments and calls into headers. The prior conservative rejection guard
has been removed and its failing differential is an active passing test.

Closure evidence: [compiler-ci #41414499660](https://github.com/bingordo/compiler-ci/actions/runs/41414499660)
passed the expanded GCC/Clang source-origin differential matrix, driver
integration, Rust Clippy and native GCC differential tests.

Resource limits are under active development (#114). The internal candidate has
an **in-engine monotonic 1,000,000 token-visit work budget** across macro rescans,
argument prescans and included files (validated by compiler-ci #42803491387).
This is now complemented by a separate **1,000,000 raw preprocessing-token
input budget**, enforced via a capped lexical iterator *before* collection into
the initial per-file token Vec. Both recursive-cycle probes and post-include guard/pragma fingerprint rescans
charge the same translation-unit-wide budget. Tests verify oversized roots,
cumulative header input and the second header fingerprint pass. This is validated
by [compiler-ci #42814316308](https://github.com/bingordo/compiler-ci/actions/runs/42814316308)
and [compiler-ci #42814806296](https://github.com/bingordo/compiler-ci/actions/runs/42814806296)
(feature-gated Rust suites, GCC/Clang differential tests, Clippy and
checker/Cranelift integration).

The 16 MiB **output** guard still runs after rcc returns its token vector.
A further **16 MiB cumulative source-byte budget** is now enforced when
reading included headers from disk (bounded reads, not check-then-read
metadata), with a separate bounded root read in the Mini-C adapter. The
preprocessor also accounts for client-preloaded and virtual root/header
sources. This passed [compiler-ci #42921431587](https://github.com/bingordo/compiler-ci/actions/runs/42921431587).

A further translation-unit-wide **16 MiB synthetic spelling budget** now
limits generated source-map text for C99 `#` stringification, `##` token
pasting and dynamic built-in macro values. Text growth is checked before
string construction and the output is charged once registered, across nested
macro rescans and included files; small-budget regression tests cover
stringification and paste. This passed [compiler-ci #42921935531](https://github.com/bingordo/compiler-ci/actions/runs/42921935531).

The merged revisions also passed a 128-seed GCC/Clang macro differential check in [compiler-ci #42922134884](https://github.com/bingordo/compiler-ci/actions/runs/42922134884) on Mini-C commit `45af40fa2e87d9759633a2011726ce695807c8b0`.

Time/peak-memory budgets and the remaining temporary argument clones are
still open. The macro-substitution temporary guard rejects replacements exceeding
the 1,000,000-token work ceiling before reserving the body vector or appending
arguments/paste results (validated by
[compiler-ci #42913501690](https://github.com/bingordo/compiler-ci/actions/runs/42913501690)).
An additional per-run 1,000,000-token **accumulated output buffer** ceiling
checks completed source lines and included-header results before appending
them (validated by
[compiler-ci #42914156513](https://github.com/bingordo/compiler-ci/actions/runs/42914156513)).
Both checks included feature-gated Rust unit tests, GCC/Clang differentials,
Clippy and checker/Cranelift integration.

These guards do not yet limit all intermediary temporary allocations or
cumulative bytes across nested include calls; they are **not** a full
memory/CPU safety guarantee.
No C23 conformance claim is made.

## Deterministic differential testing

`crates/preprocess/tests/internal_differential.rs` now generates bounded,
reproducible macro graphs using an in-tree xorshift generator with fixed
seeds. The graphs vary nested object/function macro expansions, token
pasting, stringification and conditional compilation. Each case is
compared token-for-token against **both GCC and Clang**. Failed assertions
print the exact source and seed to reproduce without requiring a fuzzing
framework or a new dependency.

The frequent `compiler-ci` checks use four seeds by default. For a
larger test, manually dispatch the
[Mini-C CI workflow](https://github.com/bingordo/compiler-ci/actions/workflows/mini-c-cranelift.yml)
with `cpp_seed_count=128`, or run locally:

```sh
MINI_C_CPP_SEED_COUNT=128 cargo test -p mini-c-preprocess \
  --features internal-rcc --test internal_differential
```

The upper bound of 128 protects the shared GitHub Actions budget. No
claim of exhaustive C99 macro conformance follows from the seeded tests;
they complement, rather than replace, the hand-written edge-case
regressions and broader source-provenance testing.

## Before enabling by default

1. Increase the GCC/Clang differential corpus substantially and minimize
   mismatches reproducibly. Test arbitrary macro interactions, trigraphs,
   comments, phase ordering, `_Pragma`, `#line` and target-specific built-ins.
2. Extend the now-correct builtin invocation-site tracking into complete
   diagnostic expansion traces, including nested synthetic `#`/`##`
   token origins and argument provenance; keep spelling spans independent.
3. **Deferred beyond CPP-C99:** evaluate C23 additions such as `__VA_OPT__`,
   `#elifdef`, `#elifndef`, `#embed` and `__has_embed` separately.
   The pinned rcc candidate implements C99; newer facilities may require
   extending the engine or reconsidering the dependency.
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
