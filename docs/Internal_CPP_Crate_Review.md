# Internal C preprocessor: existing Rust crates, selection review

Status: **candidate research, not yet adopted** (2026-11-01).
Target: a correct, library-based, no-subprocess preprocessing path for
`bingordo/mini-c`. Existing `cc -E`/`clang -E` drivers remain the default
until differential conformance gates pass.

## Requirements

- ISO C99 preprocessing as a minimum, with a clearly specified path to C23:
  translation phases (source normalisation, backslash-newline splicing,
  preprocessing tokens, comments), directives, macro invocation and rescanning,
  object/function/variadic macros, `#` and `##`, `defined`, `#if`
  evaluation and error diagnostics. For C23 additionally `__VA_OPT__`,
  `#elifdef`/`#elifndef`, `#embed` and `__has_embed`.
- Correct separate quoted/system include resolution, nested includes and
  include guards, `#pragma once` as an optimization (not implicit once-only
  behavior), `#line`, `__FILE__`/`__LINE__`, expansion/origin information,
  and conservative recursion/resource limits.
- Rust library callable independently of CLI subprocesses, bounded CPU and
  memory, no coupling to Mini-C's parser or to the native Cranelift backend.
  Avoid expanding Cargo's normal dependency tree unnecessarily.
- A token-stream API with spans is preferable; a source-string API is
  acceptable as an initial adapter only if safe token separators and source
  mapping can be retained.

## Candidate matrix (checked against documentation and source)

| Crate | Advantages | Limitations and integration risk | Initial verdict |
| --- | --- | --- | --- |
| [rcc_preprocess](https://docs.rs/rcc_preprocess/latest/rcc_preprocess/) 0.0.1 | C99 translation phases 1–4; token streams with spans; Dave Prosser hide sets for rescanning and recursion; raw-argument paste/stringize logic; #if/include handling; extensive in-tree tests, MIT/Apache-2.0 | Bound to rcc_session, rcc_span, rcc_errors, rcc_lexer, rcc_data_structures (+ transitive crates); public API is Session/FileId/PpToken rather than string-to-string; C23 features are not claimed; early version | **First prototype candidate**, behind `--cpp=internal` |
| [cinrs-core](https://docs.rs/cinrs-core/latest/cinrs_core/pp/index.html) 0.3.0 | Spanned macro-origin tokens, hide sets, macros, C23 `__VA_OPT__`, `#embed`, correct include-file source mapping and `#line`; MIT/Apache-2.0 | Whole C-to-Rust compiler frontend, integration designed around its own lexer/parser/proc_macro2 inputs; upstream explicitly disclaims API stability, and its current toolchain requirement may raise MSRV | Strong reference or alternative if C23 integration is viable |
| [includium](https://docs.rs/includium/0.1.4/includium/) 0.1.4 | Simple `process` and `PreprocessorDriver` API, no dependencies, macro/include/conditional features, MPL-2.0 | Source-string output does not preserve per-token origins and reconstructs lines without GCC-style line markers. A checked-in test expects `PASTE(pre_, VALUE)` (with `#define VALUE test`) to yield `pre_test`, while C's raw paste operands yield `pre_VALUE`; macro behavior needs deeper differential audit. No explicit `__VA_OPT__` support seen in source. | Useful zero-dependency fallback, **not safe to accept as conforming untested** |
| [ancpp](https://github.com/hemashushu/xiaoxuan-c-preprocessor) 2.2.0 | C23-oriented, spanned tokens, advanced macros and `#embed`, native file-provider abstraction; MPL-2.0 (with additional license text) | Upstream documents a restricted stringification operator, rejection of standard `#line`, and unconditional once-only inclusion even without include guards; GNU `##__VA_ARGS__` extension omitted. Additional dependencies. These are material departures from C preprocessing. | Not appropriate as a full transparent CPP replacement without patches |
| [gpp](https://docs.rs/gpp/latest/gpp/) | Generic text preprocessing | No function-like macros or #if expressions; different include syntax, recursion pitfalls | Reject |

Key standards sanity check:
```c
#define VALUE test
#define PASTE(a, b) a##b
int value = PASTE(pre_, VALUE); /* pre_VALUE, NOT pre_test */
#define EXPAND_PASTE(a, b) PASTE(a, b)
int other = EXPAND_PASTE(pre_, VALUE); /* pre_test */
```
The expected outputs are confirmed by GCC; raw operands adjacent to `##`
must not be macro-expanded before pasting. This makes the includium test
expectation a concrete correctness concern rather than a matter of style.

## First implementation proposal: reuse and isolate

1. Prototype `rcc_preprocess` as an **optional dependency** within
   `crates/preprocess`, initially enabled by `--cpp=internal`. Do not add
   `rcc` types to Mini-C syntax/sema public APIs. Design a small adapter
   exposing `Preprocessed { text, origins, dependencies }` (or spanned
   pp-tokens) and structured errors.
2. Preserve `cc -E` as default and `--cpp=none` unchanged. Map existing
   `-I`, `-D`, `-U` options into an internal configuration; reject
   unsupported switches rather than silently ignoring them. Match system
   include roots explicitly; never pretend to emulate a GCC target's full
   predefined macro universe without specifying the target.
3. Add candidate-neutral GCC/Clang **token** differential tests (never
   compare whitespace or line marker formatting), with feature gates for
   C99/C11/C23 and minimized reproducers for every failure. For diagnostics,
   verify header name, line number and macro expansion origin.
4. Test bounded recursion, malformed directives, intentionally circular
   includes, repeated unguarded headers, deep nesting, huge macro bodies,
   and failure recovery without shell invocation. Keep the 15-minute
   `compiler-ci` path cheap; run larger seeded suites in a separate
   scheduled/manual workflow.
5. Once strict C99 macro/conditional/include and mapping fixtures pass,
   switch `--cpp=internal` from experimental to supported. Decide whether
   to extend the rcc backend to C23 or reuse `cinrs-core`'s C23 engine
   based on **measured** API stability, feature coverage and CI cost.
   Only after equivalence should internal preprocessing become default.

## Mandatory acceptance corpus

- Object, function, nested and mutually recursive macros, indirect
  invocation, empty arguments, rescanning and hide-set corner cases.
- Stringification preserves raw token spelling with canonical whitespace;
  pasting handles raw operands, placemarkers and reports invalid tokens.
- Variadics, `__VA_ARGS__`, zero variadic arguments and conditional
  `__VA_OPT__` on C23.
- Multiline continuations, comments between tokens, directives in inactive
  branches, conditional integer arithmetic, `defined` and full precedence.
- Quotes versus angle includes, nested relative paths, multiple unguarded
  includes, include guards and `#pragma once`, relative file names and
  `#include_next` only where supported.
- `__FILE__`, `__LINE__`, `#line`, built-ins and diagnostics inside nested
  headers and macro expansions.
- C23 variants (`#elifdef`, `#elifndef`, `#embed`, `__has_embed`) gated
  against the correct language standard rather than silently accepted.

This document is a **source/API review, not a claim of running those crates
inside Mini-C**. Select the dependency only after a candidate prototype
compiles and the differential acceptance gate runs in compiler-ci.
