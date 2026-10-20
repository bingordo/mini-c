# mini-c

A small Rust C frontend modeled on [bingundu/oberon](https://github.com/bingundu/oberon).

## Design

The syntax AST is independent of parsing technology and later compiler phases. Like Oberon, only the syntax crate depends on LALRPOP (0.23). No runtime dependencies other than lalrpop-util are planned for the parser; the AST itself uses std only.

Reference: ISO/IEC 9899:2024 (C23), public near-final draft [WG14 N3220](https://www.open-std.org/jtc1/sc22/wg14/www/docs/n3220.pdf). This is **not** a claim of C23 conformance.

Initial AST includes translation units, declarations, functions, blocks, conditionals, loops, calls, operators and pointers. C-specific type constructors are retained, rather than mimicking Oberon syntax. Semantic analysis, preprocessing, typedef-name ambiguity, full declarators, diagnostics, and code generation are future milestones.

Run `cargo test --workspace` after adding the parser implementation.
