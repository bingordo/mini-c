# Mini-C testing and GitHub Actions plan

## Principles

Targeted tests are the default. Full suites are reserved for milestones, significant backend changes, scheduled runs and manual dispatch. Cache Cargo registry/git artifacts and build outputs. Keep the core syntax/sema jobs independent of private Git credentials and native toolchains wherever possible.

## Test layers

1. Lexer and parser: tokens, precedence, declarators, typedef shadowing, lookahead, scopes, invalid syntax, UTF-8 byte spans and error recovery. Run `cargo test -p mini-c-syntax`.
2. Semantic checker: positive/negative programs, implicit conversions, linkage, scope, lvalues, integer promotions and diagnostic spans. Run only sema tests for sema changes.
3. IR/lowering: golden CFG/SSA structure and verifier; run affected `compiler-ir` and `compiler-opt` tests when integration changes.
4. Native smoke: compile tiny programs to ELF objects, link with system cc, execute and compare to GCC reference. Include exit codes and stdout.
5. Extended differential: seeded valid programs, GCC/Clang reference at multiple optimization levels, mixed-object ABI tests and backend stress. Persist failing seeds as regressions.
6. Negative and UB policy: reject invalid syntax/typing; avoid comparing executions containing C undefined behavior. Compile reference fixtures with -fsanitize=undefined,address where practical. Do not infer equivalence from compiler crashes or timeout.

## CI triggers and jobs

- Pull requests/pushes: cached syntax job, formatting, Clippy and targeted affected-crate tests. Jobs should use path filters so docs-only changes do not rebuild everything unnecessarily; keep workflow configuration changes in the trigger list.
- After backend integration: separate native-smoke job on relevant paths, pinned Rust toolchain and Ubuntu x86-64 runner, install cc/clang only as required.
- Manual workflow_dispatch: full integration, sanitizer references, ABI matrix, deterministic fuzz seeds and documentation validation.
- Scheduled weekly extended run: broader seed ranges and optional nightly toolchain compatibility. Do not make costly extended jobs part of every commit.
- On failure: retain logs, generated C, preprocessed .i, emitted .s/.o, seed, compiler revisions, target triple and repro command as artifacts.

## Cache and credentials

Use actions/checkout@v4, dtolnay/rust-toolchain@stable and Swatinem/rust-cache@v2 for Cargo registry, Git and target caching; do not cache secrets or generated source in ways that hide changes. Keep syntax jobs public-dependency-only. If backend pin or private Oberon access requires authentication, configure narrowly scoped deploy keys/secrets only for jobs that need them; never print credentials. Prefer compiler crate access without Oberon frontend credentials. Pin Git dependency revisions and lockfile.

## Gate progression

M0: syntax tests + format + Clippy. M1: lexer/parser regression fixtures. M2: semantic positive/negative suites. M3: native smoke + GCC differential. M4-M5: memory/ABI and multi-object tests. M6: preprocessor differential fixtures. M7: scheduled fuzz and sanitizer campaigns. Every milestone must be green before being marked complete.

## Review cadence

Every commit: affected crate tests. Every milestone: full relevant suite, Clippy, a review of newly added code, and README/spec/plan accuracy. Occasionally run full workspace tests after cross-crate changes, not on every small parser edit. Use the testing strategy in bingundu/compiler as the baseline for shared-backend changes.

## Early fuzzer milestone (M1.5)

Add a deterministic Oberon-obgen-style C generator **before** native backend integration. A separate reference evaluator computes expected results without using Mini-C lowering or optimization. Generate only fully defined ISO C behavior: prevent signed overflow, zero divisors, INT_MIN/-1, invalid shifts, uninitialized reads, out-of-bounds pointers/arrays, unsequenced modifications and dependence on unspecified evaluation order. Bound loops and program size. Preserve seeds, generated source, expected result and tool versions for reproduction. Cross-check with GCC and Clang at -O0/-O2; run UBSan/ASan on reference binaries. Use small fixed seeds for push/PR and larger scheduled/manual batches, retaining regressions. Do not use -fwrapv to conceal undefined behavior.

## Diagnostic quality gate (M2.25)

Use table-driven malformed-C fixtures and assert diagnostic codes, primary spans, stable order and recovery behavior, not only rejection. Include missing punctuation, unexpected EOF, typedef scope ambiguity, invalid declarations, duplicate/unknown symbols, incompatible expressions and invalid calls/returns. Test UTF-8 and multiline locations. Run deterministic malformed-token mutation cases and preserve crash reproducers. Limit cascading errors, impose a configurable maximum, and keep core library APIs free of printing and process exits. Run targeted diagnostics tests on each relevant PR and review snapshots when wording changes.

## M1.25 external preprocessor tests

Keep external `cc -E`/`clang -E` integration tests separate from parser unit tests. Test `--cpp=none`, command failures, paths with spaces, include-file line markers, macros, conditional compilation, and diagnostics mapping. Do not shell-interpolate source paths or user flags; retain stderr and exit codes. Verify source mapping does not invent macro-expansion columns.
