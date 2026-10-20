# M1.5 deterministic C fuzzing

Run `cargo run -p mini-c-fuzz -- 42 100 > case.c`. The generator is deterministic: the seed and step count reproduce the same source. The Rust library returns `Case { seed, instructions, expected, source }`; the oracle evaluates a compact instruction sequence with checked i32 arithmetic, independent of Mini-C's AST and parser.

## Defined behavior invariants

- Only signed `int` values in [-32767,32767] are emitted, portable even on 16-bit-int targets.
- Start with an initialized local `int x = 1`; each assignment reads an initialized value.
- Addition, subtraction and multiplication are checked before emission; results outside the portable range are rejected.
- Division and remainder use positive, nonzero constants. Checked arithmetic also rejects the signed minimum divided by -1.
- There are no shifts, pointers, arrays, unsequenced modifications, unspecified-order dependencies, recursion or loops.
- The program returns 1 when its final value equals the independently calculated oracle value, and 0 otherwise. A correct generated program therefore exits with status 1; tests must check for **1**, not 0.
- No `-fwrapv` assumptions. No random behavior at runtime.

## Reference validation

Generate a fixed smoke corpus (seeds 0..63) and compile with GCC/Clang `-std=c11 -O0` and `-O2`, where installed. Execute and require exit code 1. Run UBSan/ASan reference builds when available. Persist a failing seed, step count, generated C, compiler flags, stdout/stderr and toolchain version. Do not use the Mini-C AST as the oracle.

Current scope is intentionally narrow. Extend with bounded branches, loops and calls once the parser and oracle can independently validate their behavior.
