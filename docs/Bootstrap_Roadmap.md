# Long-term Cilly + Mini-C end-to-end compiler-stack test

**Purpose: integration testing, not toolchain independence or self-hosting.**

The goal is to exercise as much of the real compiler stack as possible in one reproducible, meaningful end-to-end workload using Cilly and Mini-C. Host Rust, GCC/Clang, system assembler/linker and external preprocessing are explicitly allowed. Do not rewrite Rust components, eliminate trusted tools, or pursue stage-2/stage-3 self-hosting for this goal.

## Intended test pipeline

1. Build Cilly, Mini-C, and the main compiler using their ordinary supported build tools.
2. Select a substantial, deterministic C workload drawn from the actual compiler stack where feasible. Audit Cilly's repository and interfaces before deciding its exact role in the pipeline.
3. Feed that workload through the Cilly/Mini-C frontend path, shared compiler IR, optimizer, instruction selector, register allocator, x86 backend, and object emission/linking. Record which components were actually exercised.
4. Execute the resulting program against fixed inputs and compare with an independently built GCC/Clang reference. Compare exit status, stdout, output files and hashes where appropriate.
5. Repeat under several optimization modes and with seeded generated inputs. Capture failure artifacts, logs, source revision pins, and minimized reproducers.

## Acceptance criteria

- A documented command and CI workflow rebuild and execute the complete test reproducibly.
- The test runs **real generated native code**, not only parsing or IR validation.
- The expected result is checked independently, with no shared optimizer/backend between test and reference.
- The report identifies each compiler-stack component reached and any bypasses; no unsupported claim of complete coverage.
- A small deterministic PR smoke test is cheap; a larger scheduled/manual run expands coverage.
- Existing targeted tests remain primary for diagnosis; this test is a high-value integration gate.

## Immediate tasks

1. Locate and inspect the canonical Cilly repository, its language, interfaces and build dependencies.
2. Verify Mini-C native M3 adapter in the real compiler and extend its C coverage only as required by the selected workload.
3. Implement the smallest meaningful cross-project end-to-end program, using existing toolchains and linker.
4. Add independent reference comparison and reproducible failure capture.
5. Grow the workload toward compiling and executing substantive compiler-stack code, without creating a self-hosting requirement.

See [Implementation_Plan.md](Implementation_Plan.md) for normal Mini-C milestones. This goal is separate from ISO C completeness and should not block routine compiler development.
