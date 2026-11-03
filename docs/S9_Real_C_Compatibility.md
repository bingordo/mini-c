# S9 real-C compatibility gate

The compiler's [S9 investigation](https://github.com/bingordo/compiler/blob/main/docs/research/Generated_Rules_S9_Real_Musl_2026-10-29.md) captured twelve real musl libc function bodies at an identified upstream commit, with unmodified expression bodies and source/license provenance. Mini-C issue [#4](https://github.com/bingordo/mini-c/issues/4) tracks the corresponding language gaps.

## Reproducible frontend acceptance

Run from a Mini-C checkout with the checker already built. The twelve fixture files are retained byte-for-byte in `tests/fixtures/musl-s9`, together with the musl MIT license and a manifest of source Git blob hashes. CI checks that every file still matches its pinned hash, without requiring credentials to the private compiler repository:

```sh
cargo build -p mini-c
python3 tools/test_musl_s9.py
```

To compare against a separately available checkout of `bingordo/compiler`:

```sh
python3 tools/test_musl_s9.py --compiler-repo ../compiler
```

The script invokes `mini-c check --cpp=none` independently for each of the twelve fixtures and records every failure's return code, stdout, and stderr in `target/s9-musl-acceptance.json`. It does not rewrite the C functions, download external sources, or invoke Cargo. It rejects missing fixtures or a missing checker instead of silently weakening coverage. The gate requires unchanged `isblank` and a configurable minimum number of accepted functions; the initial floor is one. The tool **measures acceptance**, not native code execution or conformance.

To raise the regression floor after a verified implementation milestone:

```sh
python3 tools/test_musl_s9.py --compiler-repo ../compiler --min-accepted 4
```

Do not raise the threshold until the same revision has passed the checker gate and independent GCC differential runtime tests with defined inputs. Compare the accepted functions through the native compiler adapter and optional Cranelift backend separately; preserve all failures as artifacts and distinguish syntax rejection, semantic rejection, backend unsupported features, and wrong-code behavior.

## Milestone status (2026-11-03)

- **Implemented:** character constants; bitwise, shift and compound-assignment operators; prefix/postfix updates; `for`, comma and `do-while` expressions/statements; and conditional expressions `?:` with lazy branch evaluation.
- **Implemented and validated:** explicit integer casts, including `(unsigned)c`, nested casts, integer typedef type-names, signed/unsigned wrapping and narrowing. PR [#33](https://github.com/bingordo/mini-c/pull/33) merged after passing [syntax CI](https://github.com/bingordo/mini-c/actions/runs/41660398565) and [Cranelift/GCC differential CI](https://github.com/bingordo/mini-c/actions/runs/41660398548). This uses the existing typed conversion path instead of a separate backend.
- **Known boundaries:** cast type-names with pointers, `void`, and aggregates are deliberately not supported in this slice. The standalone LALRPOP expression entry point cannot resolve typedef-sensitive casts and remains limited; the incremental translation-unit parser is the supported path.
- **Next acceptance gate:** execute the twelve *unmodified* pinned S9 fixtures through `tools/test_musl_s9.py`, record accepted/total and all rejection diagnostics; run verified native compiler/Cranelift vs GCC comparisons for accepted functions. CI success on targeted analogues is not a substitute for the full corpus count.
- **Next language and ABI priorities:** address remaining real-C rejections based on that report; only expand features demonstrated to block representative code, and preserve correct type semantics. Keep unsupported conversions explicitly rejected.

Do not raise the current S9 acceptance floor above one until the pinned corpus check actually verifies a higher count. The remaining work is end-to-end native adapter validation and documented corpus coverage, not reimplementing already landed language features.

Keep the main parser incremental and both frontend crates backend-independent. Use the ordinary short target checks on every PR; reserve full compiler-ci integration for a verified vertical slice.
