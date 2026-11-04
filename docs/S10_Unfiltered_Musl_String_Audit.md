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

At this initial baseline, the incremental Mini-C parser did not accept the
valid C declaration-specifier ordering **long unsigned int**. Every rejection
occurs **before** the original musl function body is reached; 0/24 is
therefore a full-header compatibility baseline, not a measurement that
all 24 algorithms themselves are unsupported. The report retains the
first preprocessed error line and nearest #line source marker for every
file to help distinguish subsequent header failures from actual
function-body C feature gaps.

The initial Mini-C acceptance floor stays at zero. Do not turn this
coverage gate into an artificially green benchmark by dropping failures
or editing the pinned function sources.

## Second measurement: cleared stddef blocker, next header limitation

Concurrent [PR #41](https://github.com/bingordo/mini-c/pull/41)
implemented order-independent C integer declaration specifiers with
syntax regressions for equivalent spellings and invalid duplicates.
An independent [syntax CI #41703530648](https://github.com/bingordo/mini-c/actions/runs/41703530648)
and [Cranelift/GCC CI #41703530700](https://github.com/bingordo/mini-c/actions/runs/41703530700)
validated the same change on a concurrent branch (PR #42 was closed
as redundant instead of force-merging a conflicting implementation).

The S10 source fixtures remained **unchanged**. All 24 full translation
units now parse beyond the previously universal stddef.h size_t typedef.
The full-header acceptance rate remains **0/24**, but the first errors
shifted to host string-header declarations:

- **23/24** in /usr/include/string.h at the memcpy prototype
  with `__restrict`-qualified pointer parameters
- **1/24** in /usr/include/strings.h at the bcmp prototype
  with `const void *` parameters

The audit now proves that the first blocker was fixed and identifies the
next family of problems: standard `const` and `restrict` qualifiers,
and GNU header spellings/extensions. Treat header parsing independently
from the actual string-body feature coverage. It would be misleading
to report the function bodies as 24 proven frontend failures.

## Third measurement: const/restrict semantics, next GNU attribute barrier

[PR #46](https://github.com/bingordo/mini-c/pull/46) adds
qualifier-aware declarator parsing for `const`, `restrict` and GNU
`__restrict` spellings, and preserves `const` through backend-neutral
semantic types. It enforces immutable object/member lvalues, pointer
qualification and no-const-discard conversions, while keeping
`restrict` an alias-analysis promise with no generated no-alias
optimizations. It preserves concurrent S11 `long long` support.

[Syntax CI #41741709240](https://github.com/bingordo/mini-c/actions/runs/41741709240)
and [Cranelift/GCC CI #41741709183](https://github.com/bingordo/mini-c/actions/runs/41741709183)
passed on the combined implementation.

The S10 original 24 source files, manifest and GCC invocation are unchanged.
The full preprocessed translation-unit acceptance rate is still **0/24**.
All 24 now pass the formerly common `const`/`restrict` function
declarations and first fail at GNU trailing
`__attribute__ ((__nothrow__ , __leaf__))` annotations in libc headers
(23 cases in `string.h`, one in `strings.h`).
This is measurable progression through the *host headers*, not proven
acceptance of the underlying musl string-function bodies.

The next actionable compatibility milestone is structured GNU attribute
syntax for external function/object declarations, preserving meaningful
ABI/layout/calling-convention attributes and explicitly rejecting
unimplemented ones instead of indiscriminately deleting attributes.
`volatile` remains unsupported because it affects observable access
behavior; it must not be silently erased.

## Fourth measurement: parsed advisory GNU attributes, next access/extension gaps

[PR #50](https://github.com/bingordo/mini-c/pull/50) adds a **bounded,
structured parser** for trailing GNU function-declaration attributes.
It recognizes no-argument optimization/diagnostic hints such as
`__nothrow__`, `__leaf__`, and `__pure__`, plus a checked
`__nonnull__(1,2)` integer-parameter-position list. Accepted hints are
not exploited by Mini-C's optimizer or codegen; not acting on these
optional promises is conservative. Unknown annotations and ones that
could affect ABI, object layout, or control flow (`ms_abi`,
`aligned`, `packed`, `noreturn`, etc.) are **rejected rather than
silently dropped**.

[Syntax CI #41747609765](https://github.com/bingordo/mini-c/actions/runs/41747609765)
and [Cranelift/GCC differential CI #41747609819](https://github.com/bingordo/mini-c/actions/runs/41747609819)
both passed.

All **24 upstream S10 C files remain unchanged**, and the full
preprocessed-translation-unit frontend acceptance rate remains **0/24**.
The first reported rejection has moved past the formerly universal
`__attribute__((__nothrow__,__leaf__))` and
`__attribute__((__nonnull__(1,2)))` declarations:

- **23/24** now encounter GNU `__access__(__write_only__,1,4)`
  on a `string.h` prototype. Unlike the accepted harmless hints,
  this requires its own explicit policy for access/size contracts.
- **1/24**, `strcasecmp.c`, now encounters `__extension__`
  on an external declaration in `strings.h`. This C extension
  marker remains unimplemented.

The next step is an explicit representation or conservative treatment
of access-size annotations, together with scoped GNU `__extension__`
syntax, before measuring the next host-header or actual function-body
barrier. Do not quietly strip these constructs or report any untested
musl function as executable.

## Follow-up priority

Use the report's rejected/total counts to rank concrete parser and semantic
gaps: qualifiers and restrict, standard declarations and size_t, common
pointer/array operations, preprocessing dependencies, and remaining ABI
limitations. Avoid writing custom source transformations for each function.
Only perform native/GCC **execution** and optimizer size/performance
comparisons on inputs with validated object/runtime semantics; keep all
incompatible inputs in the denominator.
