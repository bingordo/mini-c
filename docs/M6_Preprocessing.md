# M6 external preprocessing contract

**Milestone status:** complete for the external preprocessing path
(November 1, 2026). Full-workspace, Clippy, GCC/Clang fixture and native
differential verification passed in
[compiler-ci run #41348660474](https://github.com/bingordo/compiler-ci/actions/runs/41348660474).
The internal full-C preprocessor is deliberately out of scope.

**Scope:** The Mini-C frontend delegates full macro expansion and header
inclusion to an external C preprocessor. No internal full-C preprocessor is
implemented. The scanner and semantic checker remain independent of subprocess
execution. The contract applies to both the semantic-check CLI and optional
Cranelift ELF object emitter.

## Commands

```sh
# Default: invoke cc -E, then parse and type check the preprocessed result.
cargo run -p mini-c -- check source.c

# Includes, defines and undefines (attached and separated forms supported).
cargo run -p mini-c -- check -I include -DFEATURE=17 -UOLD source.c

# Choose Clang or an explicit executable; forward a custom argument verbatim.
cargo run -p mini-c -- check --cpp=clang -Iinclude source.c
cargo run -p mini-c -- check --cpp=/usr/bin/clang --cpp-arg=-undef source.c

# Avoid any subprocess for input that has already been preprocessed.
cargo run -p mini-c -- check --cpp=none preprocessed.i

# Generate an x86-64 ELF object with the same external preprocessing options.
cargo run -p mini-c-cranelift -- --emit-obj module.o -Iinclude -DFEATURE=17 source.c
cargo run -p mini-c-cranelift -- --emit-obj module.o --cpp=none preprocessed.i
```

The syntax library accepts source text and never launches a preprocessor.
`--cpp=none` explicitly forbids `-I/-D/-U/--cpp-arg` because those flags
would otherwise be silently ignored. A `--` token terminates driver option
parsing; it permits input filenames starting with a hyphen. Each CPP option
is an OS-level argument, not shell syntax. The preprocessor subprocess has
null standard input; nonzero exits return the compiler's stderr. Input
filenames beginning with a hyphen receive a safe relative path prefix
where needed to avoid accidental GCC option parsing.

## Diagnostics

The preprocessor source map recognizes GCC/Clang style `# N "file" flags`
markers and `#line N "file"` directives. Escaped quote/backslash sequences in
filenames are decoded. The checker reports original file and line for
lexical, syntax, and semantic errors in header files; the native object driver
keeps structured parser/type-checker diagnostics until after source mapping.
Already-preprocessed text without external CPP retains source line/column
information. Macro expansion generally destroys original token columns, so
the driver deliberately does **not** claim precise columns after CPP.
Backend-only object emission errors may have no source span and use the
input filename instead.

## Verification and supported guarantees

- Unit tests exercise joined and separated `-I/-D/-U` flags, macro values
  with spaces/metacharacters, external compiler selection, disabled CPP
  flags, missing values, and input-file separator behavior.
- Real `cc -E` integration tests use nested, path-with-spaces include files,
  conditional branches, macro definitions and undefinitions, and a malformed
  included header to verify mapped errors.
- GCC/Clang differential fixtures compare the resulting supported C tokens,
  not compiler-specific line marker formatting.
- Negative preprocessing tests preserve nonzero exit/stderr and missing
  executable errors. A metacharacter fixture verifies the driver does not
  execute shell commands embedded in option values.
- Cranelift integration tests compile preprocessed header/macro examples into
  native ELF objects and verify mapped syntax diagnostics.
- Targeted M6 suites run in `bingordo/compiler-ci`, including the
  `mini-c-preprocess`, CLI, syntax/sema and Cranelift crates; Clippy and
  Mini-C/GCC native differential execution remain in the same CI workflow.
  See https://github.com/bingordo/compiler-ci/actions/workflows/mini-c-cranelift.yml

## Limits and follow-up scope

External tool availability, its macro semantics and the system header set
belong to the selected compiler/preprocessor, not Mini-C. The Mini-C parser
still handles only its documented C subset; normal libc headers may expand
to syntax or types it does not implement. User-provided preprocessor
executables are intentionally executable paths, and should not be treated as
untrusted data: selecting one explicitly authorizes running that executable.
Full internal preprocessing, header dependency discovery, arbitrary toolchain
compatibility, macro expansion trace-back and sandboxing of external compilers
are beyond M6's external preprocessing milestone.

The M6 exit gate is a passing normal/negative GCC/Clang fixture suite,
source-line diagnostics, direct subprocess argument handling, and a green
cross-repository CI run.
