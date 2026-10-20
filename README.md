# mini-c

A small, embeddable Rust frontend for a deliberately limited subset of C, modeled in part on [bingundu/oberon](https://github.com/bingundu/oberon).

## Implemented functionality

- Incremental lexer and translation-unit parser: each global declaration or complete function body can be consumed independently, without retaining an entire translation-unit AST.
- Basic C scalar/pointer type syntax, function definitions and prototypes, local declarations, arithmetic, comparisons, assignments, calls, blocks, if/else and while, plus scoped typedef-name classification.
- `mini-c check` diagnostic driver with filename and source-location reporting, optional externally invoked `cc -E`/`clang -E`, source-line marker handling, and a no-preprocessor path.
- Deterministic defined-behavior C corpus generator (`mini-c-fuzz`) with an independent arithmetic oracle, reference validation against GCC/Clang, sanitizer smoke tests, failure-artifact capture, and an extended weekly GitHub Actions workflow.

**This is not a conforming ISO C implementation.** Its parser recognizes only a subset; declarations, literals, language semantics, full preprocessing, type checking, and code generation remain incomplete or future work. C23 provides the language design reference ([WG14 N3220](https://www.open-std.org/jtc1/sc22/wg14/www/docs/n3220.pdf)).

## Development and smoke tests

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p mini-c -- check --cpp=none example.c
cargo run -p mini-c -- check --cpp=cc example.c
cargo run -q -p mini-c-fuzz -- 42 120 > case.c
cargo build -p mini-c-fuzz
python3 tools/fuzz_reference.py --seed-count 12 --steps 120
```

For a preprocessed file, `--cpp=none` skips external tools. For ordinary C sources, use `--cpp=cc`, `--cpp=clang`, or `--cpp=/path/to/preprocessor`; additional preprocessor options use `--cpp-arg=<argument>`. The parser API itself never invokes a preprocessor.

## Project documents

- [Language and architecture specification](docs/Specification.md)
- [Implementation plan and milestones](docs/Implementation_Plan.md)
- [Testing and CI strategy](docs/Testing.md)
- [Defined-behavior fuzzer and reproducibility](docs/Fuzzing.md)

[Targeted CI](.github/workflows/syntax.yml) runs syntax, preprocessing, driver, Clippy, and compiler-reference fuzzer checks. [Extended fuzz CI](.github/workflows/fuzz.yml) runs weekly or by manual dispatch. The Rust formatting gate is currently disabled by request.

## Embedding in bingundu/compiler

Mini-C exposes Rust library crates; the parser, preprocessing layer and fuzzer do not depend on a code-generation backend. The native compiler driver should adapt the future typed Mini-C frontend to the existing `compiler-ir`. See the [embeddable frontend contract](docs/Specification.md#embeddable-frontend-contract).
