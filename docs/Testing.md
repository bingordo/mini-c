# Mini-C testing and GitHub Actions plan

## Principles

Run one combined workspace test invocation on main pushes and pull requests rather than retesting individual crates in separate Cargo invocations. Reuse the installed runner Rust toolchain, cache Cargo dependencies plus workspace build products and incremental compilation objects, and compile the fuzz generator executable once per job. Reserve the full C compiler/sanitizer matrix for scheduled/manual extended fuzz runs. Keep the core syntax/sema jobs independent of private Git credentials and native toolchains wherever possible.

## Test layers

1. Lexer and parser: tokens, precedence, declarators, typedef shadowing, lookahead, scopes, invalid syntax, UTF-8 byte spans and error recovery. Run `cargo test -p mini-c-syntax`.
2. Semantic checker: positive/negative programs, implicit conversions, linkage, scope, lvalues, integer promotions and diagnostic spans. Run only sema tests for sema changes.
3. IR/lowering: golden CFG/SSA structure and verifier; run affected `compiler-ir` and `compiler-opt` tests when integration changes.
4. Native smoke: compile tiny programs to ELF objects, link with system cc, execute and compare to GCC reference. Include exit codes and stdout.
5. Extended differential: seeded valid programs, GCC/Clang reference at multiple optimization levels, mixed-object ABI tests and backend stress. Persist failing seeds as regressions.
6. Negative and UB policy: reject invalid syntax/typing; avoid comparing executions containing C undefined behavior. Compile reference fixtures with -fsanitize=undefined,address where practical. Do not infer equivalence from compiler crashes or timeout.

## CI triggers and jobs

- Pushes to main and pull requests targeting main: a **single** cached Rust workspace test invocation, Python harness unit tests, workspace Clippy, then one generator binary build and four short seeds using one GCC `-O2` and one Clang `-O0` executable per seed (8 reference compilations total). The formatting gate remains disabled by request. Only code/tools/CI workflow changes trigger this job; documentation-only changes do not.
- After backend integration: separate native-smoke job on relevant paths, pinned Rust toolchain and Ubuntu x86-64 runner, install cc/clang only as required.
- Manual workflow_dispatch: full integration, sanitizer references, ABI matrix, deterministic fuzz seeds and documentation validation.
- Scheduled weekly extended run: `.github/workflows/fuzz.yml` rotates through 16 seeds of 240 steps with GCC/Clang at O0/O2 plus the sanitizer variants (96 reference compilations per week). The manual dispatch defaults to 64 seeds / 300 steps and supports custom seed start/count/steps. The extended job **does not rerun** the workspace tests, Python unit tests or Clippy; those belong to the regular CI gate. Only changes to the extended workflow itself cause an extra verification push run.
- On failure: retain logs, generated C, preprocessed .i, emitted .s/.o, seed, compiler revisions, target triple and repro command as artifacts.

## Cache and credentials

Use the Rust/Cargo binaries already present on the Ubuntu 24.04 runner; avoid downloading/installing a duplicate toolchain on each job. `actions/cache@v4` restores `~/.cargo/registry`, `~/.cargo/git` and `target/debug` (including the workspace's binaries and `target/debug/incremental`). Set `CARGO_INCREMENTAL=1`, and key the cache by operating system, rustc release, the workspace manifests/lockfile and commit SHA. Prefix restore keys permit later commits to **incrementally reuse** objects produced by earlier commits, while Cargo fingerprints and compiler-version keys determine whether an object is still valid. Both CI workflows use the same cache prefix, so the extended job can reuse a generator built by regular CI. Avoid `rust-cache` for this project while workspace incremental artifacts and binaries are required: its normal cache cleanup drops incremental objects and workspace crates.

Each commit creates at most one fresh cache entry per workflow; GitHub cache retention/eviction bounds accumulated entries. Do not cache fuzz failure output or generated C executables. A single restored generator binary is executed for every seed in a job. The full C reference matrix necessarily compiles distinct generated C sources, but only the scheduled/manual workflow performs all six compiler/flag variants per seed.

Keep these jobs public-dependency-only. If a future backend needs private Oberon access, scope credentials narrowly to the affected job and never cache secrets. Pin Git dependencies and add a checked-in Cargo.lock when the dependency graph is stabilized.

## Gate progression

M0: syntax tests + format + Clippy. M1: lexer/parser regression fixtures. M2: semantic positive/negative suites. M3: native smoke + GCC differential. M4-M5: memory/ABI and multi-object tests. M6: preprocessor differential fixtures. M7: scheduled fuzz and sanitizer campaigns. Every milestone must be green before being marked complete.

## Review cadence

Every main-branch commit or pull request: one fast cached workspace test run and Clippy. Every milestone: review newly added code and README/spec/plan accuracy, and run extended differential/sanitizer tests when coverage changes. Avoid duplicating a just-passed workspace test run in the extended fuzz workflow. Use the testing strategy in bingundu/compiler as the baseline for shared-backend changes.

## Early fuzzer milestone (M1.5)

The implemented `mini-c-fuzz` crate now emits checked signed arithmetic, guarded branches and bounded loops. `tools/fuzz_reference.py` compares GCC and Clang at -O0/-O2, with GCC UBSan and Clang ASan+UBSan. Four short, nonsanitized seeds run in the normal syntax workflow. A weekly extended run rotates through 16 longer seeds; manual dispatch defaults to 64 seeds. The Python harness has its own regression tests. The generator, C, compiler flags, logs and metadata are retained on failure under `target/fuzz-artifacts/` and uploaded by GitHub Actions. See [Fuzzing](Fuzzing.md).


Add a deterministic Oberon-obgen-style C generator **before** native backend integration. A separate reference evaluator computes expected results without using Mini-C lowering or optimization. Generate only fully defined ISO C behavior: prevent signed overflow, zero divisors, INT_MIN/-1, invalid shifts, uninitialized reads, out-of-bounds pointers/arrays, unsequenced modifications and dependence on unspecified evaluation order. Bound loops and program size. Preserve seeds, generated source, expected result and tool versions for reproduction. Cross-check with GCC and Clang at -O0/-O2; run UBSan/ASan on reference binaries. Use small fixed seeds for push/PR and larger scheduled/manual batches, retaining regressions. Do not use -fwrapv to conceal undefined behavior.

## Diagnostic quality gate (M2.25)

Use table-driven malformed-C fixtures and assert diagnostic codes, primary spans, stable order and recovery behavior, not only rejection. Include missing punctuation, unexpected EOF, typedef scope ambiguity, invalid declarations, duplicate/unknown symbols, incompatible expressions and invalid calls/returns. Test UTF-8 and multiline locations. Run deterministic malformed-token mutation cases and preserve crash reproducers. Limit cascading errors, impose a configurable maximum, and keep core library APIs free of printing and process exits. Run targeted diagnostics tests on each relevant PR and review snapshots when wording changes.

## M1.25 external preprocessor tests

Keep external `cc -E`/`clang -E` integration tests separate from parser unit tests. Test `--cpp=none`, command failures, paths with spaces, include-file line markers, macros, conditional compilation, and diagnostics mapping. Do not shell-interpolate source paths or user flags; retain stderr and exit codes. Verify source mapping does not invent macro-expansion columns.
