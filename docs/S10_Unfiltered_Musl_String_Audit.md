# S10 — unfiltered real-world musl string-source acceptance

S10 is a **source-coverage audit**, not a conformance or runtime-performance
benchmark. It retains **all** selected examples and every rejection.

## Selection, provenance, and license

The source is the upstream [musl repository](https://github.com/ifduyue/musl)
at fixed commit 7813e9119eb5fe681b6750b8a033a0490bca0941,
the same commit used in [S9](S9_Real_C_Compatibility.md).

Selection was fixed **before testing Mini-C**:

1. List all src/string/*.c files at the pinned musl commit.
2. Sort file basenames lexicographically.
3. Select the **first 24**, irrespective of syntax, complexity, or
   expected compiler support.

The full original files (including includes, macros, declarations, and
function bodies) live in tests/fixtures/musl-s10/string/, unchanged from
upstream; their Git blob SHA-1 values are recorded in
tests/fixtures/musl-s10/manifest.json. The original musl
copyright/license notice is retained in UPSTREAM-LICENSE.txt.
The checker rejects any missing/extra/modified .c fixtures.

This is a **different and substantially harder source form** than S9's
header-free extracted function bodies; acceptance rates are not directly
comparable.

## Reproduce and interpret

From the Mini-C repository root:

~~~sh
cargo build -p mini-c
python3 tools/test_musl_s10.py \
  --checker target/debug/mini-c \
  --report target/s10-musl-string-acceptance.json
~~~

The audit attempts **every** selected file through independent stages:

- GCC -fsyntax-only -std=gnu11 -D_GNU_SOURCE on the *original complete source*
  provides host-compiler acceptance, independent of Mini-C.
- Host cc -E -std=gnu11 -D_GNU_SOURCE processes the original file and its
  include directives (no Mini-C-specific edits).
- Mini-C check --cpp=none checks **only** successfully preprocessed text;
  preprocessing failures remain reported rather than being discarded.
- Optionally, --backend target/debug/mini-c-cranelift attempts native
  ELF object emission **only for** sources accepted by Mini-C's checker.
  There is no claim of executable/ABI correctness from object emission.

The JSON report contains all 24 file names, each original Git blob SHA-1,
stdout/stderr and return codes (or a recorded timeout/error), counts for
every stage, and exclusive blocking-stage categories. The CI coverage
test requires **24/24 inputs accounted for** and correct retained source
hashes; the initial --min-accepted default is zero. It does **not** assert
that an unimplemented feature must suddenly compile.

The published result is constrained by the **host's system headers**,
usually glibc rather than musl's own build headers. A header declaration
rejection is not evidence that the body itself uses an unsupported language
construct. The next investigation must separate system-header barriers
from function-body syntax and semantics, without hiding initial failures.

CI: .github/workflows/syntax.yml uploads the complete unfiltered report as
mini-c-s10-unfiltered-string. The original 12/12 S9 gates remain unchanged,
ensuring a known native-compatible control group.

## First measured baseline (2026-11-03)

The first [S10 syntax CI run #41702023466](https://github.com/bingordo/mini-c/actions/runs/41702023466)
completed successfully, including the independent reference-harness tests.
The unfiltered cohort produced:

| Metric | Outcome |
| --- | ---: |
| Original upstream full translation units selected and SHA-verified | 24/24 |
| Host GCC syntax accepted | 24/24 |
| Host C preprocessing completed | 24/24 |
| Mini-C syntax/semantic frontend accepted (whole preprocessed units) | **0/24** |
| Reported Mini-C frontend rejections | **24/24** |

The [final syntax CI #41702729328](https://github.com/bingordo/mini-c/actions/runs/41702729328)
confirmed the same totals with the diagnostic-origin instrumentation.
**All 24 first errors map to GCC's system header stddef.h**, at the
same declaration:

~~~c
typedef long unsigned int size_t;
~~~

The incremental Mini-C parser currently does not accept the fully valid
C declaration-specifier ordering **long unsigned int**. Every rejection
occurs **before** the original musl function body is reached; 0/24 is
therefore a full-header compatibility baseline, not a measurement that
all 24 algorithms themselves are unsupported. The report retains the
first preprocessed error line and nearest #line source marker for every
file to help distinguish subsequent header failures from actual
function-body C feature gaps.

The initial Mini-C acceptance floor stays at zero. Do not turn this
coverage gate into an artificially green benchmark by dropping failures
or editing the pinned function sources.

## Next blocker isolated by integer-specifier support (2026-11-03)

The incremental parser now accepts C's equivalent type-specifier orders,
including `long unsigned int`, without adding another primitive type or
rewriting upstream inputs. It continues to reject duplicated and
contradictory combinations.

[Syntax CI #41703530648](https://github.com/bingordo/mini-c/actions/runs/41703530648)
and [Cranelift/GCC CI #41703530700](https://github.com/bingordo/mini-c/actions/runs/41703530700)
passed on the parser fix. S10 still reports **0/24 complete translation
units accepted** because the entire host system header is intentionally
included, but its **first error moved past stddef.h** in all 24 files:

- **23/24:** first rejection in `/usr/include/string.h`, at the
  `memcpy` declaration with GNU `__restrict` pointer qualifiers
- **1/24:** first rejection in `/usr/include/strings.h`, at the
  `bcmp` declaration using GNU `__s1` and related declarations

This demonstrates an actual semantic-compatible parser improvement
without misrepresenting the corpus acceptance number. Next investigate
general C type qualifiers and GNU header declarations; avoid
per-function parser overrides or silently dropping system declarations.

## Follow-up priority

Use the report's rejected/total counts to rank concrete parser and semantic
gaps: qualifiers and restrict, standard declarations and size_t, common
pointer/array operations, preprocessing dependencies, and remaining ABI
limitations. Avoid writing custom source transformations for each function.
Only perform native/GCC **execution** and optimizer size/performance
comparisons on inputs with validated object/runtime semantics; keep all
incompatible inputs in the denominator.
