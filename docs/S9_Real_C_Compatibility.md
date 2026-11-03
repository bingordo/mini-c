# S9 real-C compatibility gate

The compiler's [S9 investigation](https://github.com/bingordo/compiler/blob/main/docs/research/Generated_Rules_S9_Real_Musl_2026-10-29.md) captured twelve real musl libc function bodies at an identified upstream commit, with unmodified expression bodies and source/license provenance. Mini-C issue [#4](https://github.com/bingordo/mini-c/issues/4) tracks the corresponding language gaps.

## Reproducible frontend acceptance

Run from a Mini-C checkout with the checker already built and a **local** checkout of `bingordo/compiler`:

```sh
cargo build -p mini-c
python3 tools/test_musl_s9.py --compiler-repo ../compiler
```

The script invokes `mini-c check --cpp=none` independently for each of the twelve fixtures and records every failure's return code, stdout, and stderr in `target/s9-musl-acceptance.json`. It does not rewrite the C functions, download external sources, or invoke Cargo. It rejects missing fixtures or a missing checker instead of silently weakening coverage. The gate requires unchanged `isblank` and a configurable minimum number of accepted functions; the initial floor is one. The tool **measures acceptance**, not native code execution or conformance.

To raise the regression floor after a verified implementation milestone:

```sh
python3 tools/test_musl_s9.py --compiler-repo ../compiler --min-accepted 4
```

Do not raise the threshold until the same revision has passed the checker gate and independent GCC differential runtime tests with defined inputs. Compare the accepted functions through the native compiler adapter and optional Cranelift backend separately; preserve all failures as artifacts and distinguish syntax rejection, semantic rejection, backend unsupported features, and wrong-code behavior.

## Milestone priorities (as of 2026-11-03)

- **Already implemented:** integer C character constants, bitwise operators, shifts, compound assignments, prefix/postfix updates, `for`, comma expressions, and `do-while`. These should not be counted as outstanding work merely because issue #4 predates their implementation.
- **Next major vertical slice:** C explicit integer casts with typedef-sensitive type-name disambiguation, integer promotions and signed/unsigned conversions. Include `(unsigned)c`, nested casts and representational boundary cases. Preserve target integer widths in both semantic analysis and Cranelift lowering; use GCC as the differential oracle.
- **Following slice:** conditional expressions `?:`, with correct precedence/right associativity, common result type and conversion, and strict evaluation of only the selected branch.
- **Exit criteria:** rerun all twelve pinned S9 functions unchanged and report accepted/total, per-function diagnostics, and cross-backend runtime comparisons (including signed/unsigned edge conditions). Do not claim complete support simply from a parser acceptance count.

Keep the main parser incremental and both frontend crates backend-independent. Use the ordinary short target checks on every PR; reserve full compiler-ci integration for a verified vertical slice.
