# Mini-C language and architecture specification

Status: implementation target, not a claim of ISO conformance. Reference: ISO/IEC 9899:2024 (C23); publicly available WG14 N3220 draft. Explicit exclusions and deviations must be documented with tests.

## Goals

Implement a small practical C frontend in minimal-dependency Rust. Follow the phase separation of bingundu/oberon: syntax AST, semantic analysis, and lowering. Reuse LALRPOP 0.23 only inside the syntax crate. Lower checked functions into bingundu/compiler's typed CFG SSA (`compiler-ir`), then reuse `compiler-opt`, `compiler-backend`, and `compiler-backend-x86`. Keep the AST frontend-specific and independent of backend crates. No separate C machine-code generator.

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

Prefer existing `compiler-ir` typed CFG SSA; do not invent a second shared IR. Preserve signed overflow behavior required by C (undefined, not Oberon wraparound), evaluation order and short-circuiting. Extend IR/backend deliberately for pointers, 64-bit operations, calls, globals, relocations and aggregates. Keep x86-specific ABI, instruction selection and object writing in backend crates. Initial output: x86-64 SysV ELF64 relocatable objects, linked by system cc. Optional Cranelift adapter consumes compiler-ir later, rather than Mini-C AST.

## Dependencies and portability

Only syntax crate depends on LALRPOP/lalrpop-util; standard library elsewhere wherever possible. Backend crates may be Git dependencies pinned to a revision; record reproducible pins and their update process. Never duplicate optimizer or register allocator in Mini-C. Prefer explicit diagnostic errors for unsupported targets and C features.

## Acceptance

Each milestone requires positive and negative parser tests, semantic tests, executable GCC differential cases, and targeted CI gates. Record supported C features and limitations in README; no milestone is complete based solely on successful compilation.
