# Mini-C language and architecture specification

Status: implementation target, not a claim of ISO conformance. Reference: ISO/IEC 9899:2024 (C23); publicly available WG14 N3220 draft. Explicit exclusions and deviations must be documented with tests.

## Goals

Implement a small practical C frontend in minimal-dependency Rust. Follow the phase separation of bingundu/oberon: syntax AST, semantic analysis, and lowering. Reuse LALRPOP 0.23 only inside the syntax crate. Lower checked functions directly into bingordo/compiler's TypeId-owned CFG SSA (`compiler-ir`), then reuse `compiler-opt`, `compiler-backend`, and `compiler-backend-x86`. Keep the AST frontend-specific and independent of backend crates. No separate C machine-code generator.

## Supported language, staged

M1: translation units; int/void functions, prototypes, parameters, local and global declarations; blocks, return, if/else, while, for, break, continue; decimal integer constants; arithmetic, comparison, logical, assignment and calls. Preserve C operator precedence, short-circuit evaluation and scope rules. Initially support a single translation unit and reject unsupported constructs explicitly.

M2: typedef, shadowing, qualifiers, signed/unsigned integer types, integer promotions, usual arithmetic conversions, constant expressions, casts, sizeof, arrays, pointers, indexing and address/dereference operators. Track lvalue vs rvalue, conversions, alignment, and source spans.

M3: externally linked functions, global storage, string literals, struct/union/enum declarations and layout, field access, initializer lists and multiple translation units. Support the x86-64 SysV ABI for the implemented type set.

Deferred: full C23 library, complex/atomic types, bit-fields, VLAs, designated initializers, flexible array members, _Generic, inline assembly, implementation-defined extensions, and complete C23 preprocessing. Unsupported constructs must yield diagnostics rather than silently miscompile.

## Typedef-aware lexing

Maintain a scoped ordinary-identifier namespace with bindings Typedef and Ordinary; tags and members use separate namespaces. The lexer queries this table to emit TYPEDEF_NAME or IDENTIFIER. Declaration actions update bindings at C's point of declaration; scope entry/exit and parameter scopes are explicit. Validate LALRPOP lookahead behavior with tests for shadowing, declarator ambiguity, comma-separated declarators, and typedef names reused as ordinary identifiers. Avoid global mutable state; keep lexer and parse context per translation unit.

## Preprocessing

Default to external `cc -E` or configured GCC/Clang preprocessor; support include paths, definitions, diagnostics, and `#line` mapping. Provide `--cpp=none` for preprocessed input and isolated parser tests. An internal preprocessor is optional and never presumed conformant; design a preprocessing abstraction without imposing additional dependencies on the AST or semantic phases. Do not execute untrusted compiler flags through a shell; invoke a subprocess with an argument vector.

## AST and checked representation

AST owns TranslationUnit, Item, Decl, Function, Stmt, Expr, Type, Span; preserve declarator structure where required to represent C binding correctly. Keep parsing unresolved. Semantic analysis resolves identifiers and typedefs, performs type checking and implicit conversions, computes layout and constants, and emits a checked function representation. Lowering only accepts checked functions. Expose parse_translation_unit, check_translation_unit, and lower_function entry points with structured diagnostics and source locations.

## IR/backend contract

The production `bingordo/compiler` adapter builds canonical `TypeId`/`TypedSsaModule` CFG SSA directly from Mini-C checked items. It must never construct intermediate legacy `compiler_ir::Function` SSA, use `from_legacy`/`to_legacy`, invoke legacy optimizer passes or dispatch to a legacy backend. The Mini-C AST, semantic `CType`, checked expressions, lexical bindings and C-specific qualifiers remain frontend-owned: they are not a competing compiler IR. Preserve signed overflow behavior required by C (undefined, not Oberon wraparound), evaluation order and short-circuiting. Extend IR/backend deliberately for pointers, 64-bit operations, calls, globals, relocations and aggregates. Keep x86-specific ABI, instruction selection and object writing in backend crates. Initial output: x86-64 SysV ELF64 relocatable objects, linked by system cc. The optional Cranelift reference backend consumes Mini-C checked items independently; it is not part of the production compiler path, and it must not require legacy compiler IR.

## Dependencies and portability

Only syntax crate depends on LALRPOP/lalrpop-util; standard library elsewhere wherever possible. Backend crates may be Git dependencies pinned to a revision; record reproducible pins and their update process. Never duplicate optimizer or register allocator in Mini-C. Prefer explicit diagnostic errors for unsupported targets and C features.

## Acceptance

Each milestone requires positive and negative parser tests, semantic tests, executable GCC differential cases, and targeted CI gates. Record supported C features and limitations in README; no milestone is complete based solely on successful compilation.

## Embeddable frontend contract

Mini-C must be usable as a Rust library from `bingordo/compiler` without spawning a CLI or requiring a backend. The dependency graph is one-way: `mini-c-sema` depends on `mini-c-syntax`; the standalone `mini-c` CLI and optional `mini-c-cranelift` reference depend on the frontend crates. The compiler-owned adapter in `bingordo/compiler/tools/mini-c-driver` depends on `mini-c-syntax` and `mini-c-sema`, not the reverse. Core Mini-C crates must never depend on `compiler-ir` or native backend crates. The `compiler` driver selects the checked Mini-C frontend and uses the same TypeId optimization/native backend pipeline as Oberon. Do not introduce a cyclic dependency.

Current embedding contract: a streaming `mini_c_syntax::parser::Parser`
(`ItemSource`) feeds `mini_c_sema::Checker`, which produces checked
`CheckedItem` declarations/functions. The compiler-owned adapter lowers these
directly to canonical `TypeId` types and `TypedSsaModule`/`TypedFunction`
values. There is no `compiler_ir::Function` bridge or legacy IR import/export.
Keep source-file identifiers, spans, target ABI/layout, and diagnostics explicit;
avoid global parser state. No process exit, printing, filesystem reads,
subprocess invocation, or hidden global mutable state in the core library.
A separate CLI crate owns files, external preprocessing, diagnostics rendering and command-line parsing. Library tests must run without an installed C compiler. Prefer narrow stable public types; keep generated parser internals private.

Integration strategy: pin a Git revision of Mini-C from the compiler driver, or later publish crates; keep a feature-free syntax/sema path. The compiler-owned bridge lives in `bingordo/compiler/tools/mini-c-driver`, preserving Mini-C's independence from compiler internals; new compiler integration belongs there rather than in the reference Cranelift crate. Document compatibility and version bumps when checked representations change.

## Production and reference backend boundary

Mini-C stays an independent syntax and semantic frontend. **All production
native compilation in `bingordo/compiler` flows from checked Mini-C items
directly through TypeId SSA**, its verifier, optimizer and target backends.
There must be no old `Function` IR, legacy SSA translation or silent fallback,
including when introducing FP, aggregates, GNU extensions or atomics.
Unsupported constructs are rejected with diagnostics.

Keep `mini-c-cranelift` as an optional, independent checked-frontend reference
backend. It must remain available for executable differential validation
against GCC/Clang and the native TypeId compiler, but never become a production
compiler IR bridge or a dependency of the core syntax/sema crates.

## Cranelift reference backend milestone

Before integrating the new `bingundu/compiler` native backend, add an optional Cranelift code-generation crate that consumes the checked Mini-C representation. This is a standalone executable reference path, not the shared compiler IR and not a dependency of syntax or sema. Adapt implementation patterns from Oberon's Cranelift backend while preserving C integer and control-flow semantics. Test generated objects against GCC, then reuse the same corpus when integrating `compiler-ir` and its x86 backend. See milestone M2.5 in the [implementation plan](Implementation_Plan.md).

## Defined-behavior generated test programs

The early M1.5 fuzzer is modeled on Oberon's self-checking generator but produces ISO C programs with defined behavior by construction. It uses an independent expected-value oracle and excludes signed overflow, invalid division/shifts, uninitialized reads, invalid pointer access, unsequenced conflicting side effects, out-of-bounds indexing and unspecified-order dependencies. Its valid subset and assumptions are documented and extended alongside compiler support; see [M1.5](Implementation_Plan.md).

## Diagnostic hardening milestone

M2.25, after semantic analysis and before executable backends, tests and improves syntax and semantic error handling. Library APIs must provide deterministic structured errors with accurate source spans, useful recovery, bounded cascades, and no panics on malformed input. See [Implementation Plan](Implementation_Plan.md) and [Testing](Testing.md).

## External preprocessing integration (M1.25)

Raw C files use a configurable external preprocessor as an early standalone driver layer; already-preprocessed text is accepted without subprocesses. Preserve `#line` source origin mapping for diagnostics. No dependency on a system C compiler is permitted in core parser/sema tests. The internal preprocessor remains a separate M6 task.
