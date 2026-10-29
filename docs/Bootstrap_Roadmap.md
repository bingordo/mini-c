# Long-term compiler-stack bootstrap roadmap

Status: **strategic goal, not implemented**. This complements [Implementation_Plan.md](Implementation_Plan.md).

## Goal

Bootstrap the complete compiler stack using **Cilly and Mini-C**, with a reproducible path from a documented minimal trusted toolchain to rebuilding the frontend, shared optimizer, native backends, runtime, and build utilities. Cilly's exact role, source language, and current capabilities must be audited before selecting the bootstrap architecture. Do not assume Cilly can compile Rust or that Mini-C is self-hosting.

## Stages and acceptance gates

1. **Inventory and dependency graph.** Locate the canonical Cilly implementation and its build/runtime dependencies. Inventory Mini-C, compiler, generators, runtime and CI tools; classify each component by implementation language, generated artifacts, required host tools and build order. Publish the initial trusted computing base.
2. **Practical Mini-C.** Finish the native M3 validation and M4/M5 language, ABI, memory and object-file coverage needed to compile substantial real C projects. Expand defined-behavior differential testing, with clear unsupported-feature errors.
3. **Cilly integration experiment.** Define an explicit intermediate representation or source-level handoff between Cilly, Mini-C and the shared compiler backend. Prototype a tiny deterministic program, compare results against GCC/Clang, and measure required C features. Prefer adapters and shared infrastructure to duplicate compilers.
4. **Build the C subset of the stack.** Compile C runtime/support libraries and any C-based generators with Mini-C, initially using an external linker/preprocessor if necessary. Record hashes and run native and reference binaries.
5. **Address Rust implementation dependency.** Mini-C and the main compiler currently contain Rust code. Choose and document one realistic route: a maintained C implementation of the minimal bootstrap compiler, a verified Rust-to-C translation pipeline if feasible, or a staged bootstrap that explicitly retains a seed Rust compiler. Merely compiling C inputs is not a self-hosted bootstrap.
6. **Close the build loop.** Build a stage-1 toolchain with the declared seed, rebuild stage-2 from stage-1, then stage-3 from stage-2. Compare normalized artifacts or execute differential/reproducibility tests when byte identity is not yet possible. Document unavoidable external tools, ABI and target assumptions.
7. **Reduce the seed.** Gradually eliminate dependencies on host GCC/Clang, Rust compiler, linker and external preprocessing only when verified replacements exist. Preserve a working recovery build and cross-compiler tests.

## Design principles

- Maintain a **working compiler** at every stage; bootstrap experiments cannot block normal Mini-C milestones.
- Prefer reusable compiler IR, instruction generation, ABI descriptions and optimizer passes over bespoke Cilly-only paths.
- Keep generators' source, generated output and provenance clearly separated.
- Pin source revisions, compiler versions and dependency hashes; use cached binaries and targeted CI tests.
- Validate correctness before performance. Include cross-stage executable tests, miscompilation minimization and build reproducibility.
- Track a precise distinction between **C compiler completeness**, **compiling the C portions of the stack**, **compiler self-hosting**, and **full toolchain bootstrapping**.

## Immediate next steps

- Verify M3 native integration with an actual successful targeted CI run.
- Add division/remainder, unsigned/wide integer, pointers and memory lowering in vertical slices, with explicit unsupported cases.
- Identify Cilly's repository, intended architecture and dependencies; write a concrete integration proposal after inspection.
- Create a machine-readable component/dependency inventory and minimal bootstrap smoke test.
