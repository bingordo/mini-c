# M5 closure: linkage, aggregate object model and ABI validation

Status: **complete for the supported Mini-C / Cranelift reference subset**
(November 1, 2026). This is not a claim of full ISO C or complete
SysV aggregate ABI coverage.

## Implemented

- x86-64 LP64 ELF object emission for internal/external scalar and aggregate
  global objects, tentative definitions, imports, symbol relocations and
  cross-translation-unit links against GCC objects.
- Narrow ASCII C strings with standard supported escapes, terminating NUL,
  local/global pointers, deduplicated local read-only objects, and character
  arrays initialized directly from strings. Inferred, exact, and padded
  character array bounds have tests.
- Struct and union definitions, references, forward declarations and
  separate lexical tag scopes. Types retain nominal identity. Recursive
  pointers work without embedding infinite-size types. Duplicate members,
  incompatible declarations and incomplete by-value members are rejected.
- LP64 aggregate field offsets, natural alignment and tail padding; unions
  overlay members at offset zero. Dot, arrow, nested aggregates, arrays of
  aggregates, global data and local member reads/writes are lowered.
- Struct/union local assignment and copy-initialization, with explicit
  representation copies. Positional braced aggregate initializers support
  nested arrays/structs, union first-member initialization, omitted zero
  elements, and array-bound inference. Global aggregate initializers emit
  bytes plus independently tracked ELF relocations for pointer fields and
  strings (including self references).
- Enumerators with implicit and explicit integer constant values, signed
  values in the supported `int` range, namespaced/scoped tags and ordinary
  enumerator constants. Completed enum tags use the ordinary scalar ABI.
- SysV x86-64 scalar integer/pointer calls and return values are interoperable
  with GCC. Passing or returning a struct/union **by value** is explicitly
  rejected by the backend; no silent or incorrect ABI lowering is allowed.

## Verification

- `crates/syntax`: scanner, incremental parser, typedef scope, tag and
  recursive/nested initializer syntax tests.
- `crates/sema`: object and pointer typing, tag identity, incomplete type
  errors, enum constant evaluation, aggregate layout, conversions and
  initializer bounds.
- `crates/cranelift`: native ELF compile smoke tests, including global
  relocations and negative aggregate-ABI probes.
- `tools/test_cranelift.py`: Mini-C versus GCC executable-result
  differentials and mixed-object ABI tests in both directions (scalar,
  pointer, aggregate layout, union, enum, tag recursion, string relocations).
- `bingordo/compiler-ci` runs targeted checks on changes, cached and
  scheduled every 15 minutes; milestone closure also used one full
  `cargo test --workspace` run.

Milestone closure evidence: compiler-ci run
https://github.com/bingordo/compiler-ci/actions/runs/41309708850
(full workspace tests, Clippy, GCC differential, mixed ABI).

## Deliberately unsupported or deferred

The following are not silently accepted as correct C behavior:

- SysV by-value aggregate arguments and returns (requires register class
  classification, splitting, hidden sret, stack and varargs interactions).
- Floating-point types, bitfields, packed/aligned attributes, flexible array
  members, anonymous aggregate member promotion, and full qualifier rules.
- Designated initializers, unrestricted brace elision, compound literals,
  and all generalized C initializer-expression forms.
- Wide/Unicode strings, arbitrary execution character sets, adjacent
  string literal concatenation, and full escape/encoding semantics.
- Nonstandard enum underlying types or enumerator values outside supported
  signed `int` range; full enum compatibility and prototype scope nuances.
- General relocation expressions/addends beyond named objects, strings and
  the supported positional aggregate pointer fields.
- The rest of the C language still missing from the restricted Mini-C
  frontend, independent of M5 (for example, other statement and operator
  forms).

These are follow-up scope for broader C completeness and ABI work rather
than evidence that the tested M5 reference subset implements them. Any
future support should extend common type/layout, expression and relocation
frameworks rather than adding per-syntax backend special cases.

## Constraints

The current reference target is native x86-64 Linux, LP64, ordinary natural
alignment. Data and aggregate initialization emission has an intentional
1 MiB size guard to keep differential/fuzz CI bounded. By-value aggregate
ABI tests assert a diagnostic rather than ABI interoperability.
