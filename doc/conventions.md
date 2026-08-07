# LazyCassandra — Conventions

Companion to [architecture.md](architecture.md). That document says what the
code is; this one says how it should read. Coding rules that are structural —
error types, crate boundaries, lint settings — live in architecture.md §12 and
are not repeated here.

**Last updated:** 2026-08-07

---

## 1. Comments

**Assume an expert Rust reader.** Nobody working in this codebase needs `?`,
lifetimes, `impl Trait`, or a builder chain explained to them. A comment that
narrates idiomatic Rust is noise, and worse, it trains readers to skip comments
— which means the one comment that mattered gets skipped too.

**A comment is a failure to express something in code.** Before writing one,
try the alternatives in this order:

1. Rename the variable. `pk_cols` → `partition_columns`.
2. Extract and name the function. A well-named `fn` is a comment the compiler
   checks and the refactorer cannot leave stale.
3. Introduce a type. `Posture` says more than a `bool` plus a paragraph.

Only when none of those can carry the meaning does a comment earn its place.

**What comments are for.** Things absent from the code and unavailable to the
reader by inspection:

- **Edge cases** — the non-obvious input that motivated a branch, especially
  when the branch looks redundant.
- **Past bugs** — why this is written the awkward way, so nobody "cleans it up"
  back into the bug. Reference the issue or commit.
- **Upstream defects** — a workaround for a driver, `ratatui`, or `tokio` bug.
  Name the project, the version, and the upstream issue, so the workaround can
  be deleted when it's fixed rather than outliving it.
- **Protocol and Cassandra semantics** — behaviour dictated by CQL, the native
  protocol, or a coordinator, that the code must obey but cannot explain.
- **Deliberate non-obvious choices** — the faster algorithm that looks wrong,
  the ordering that matters, the allocation that was measured.

**What they are not for.** Restating the signature. Section banners inside a
function that should have been split. Commented-out code — that is what git is
for. `// TODO` without an issue reference; either file it or fix it.

**Doc comments are a separate thing and not covered by this.** `lc-domain` is
the crate everything else is written against, and its public items carry `///`
docs (architecture.md §12). Documenting an API contract is not the same as
narrating an implementation — the rules above constrain the latter only.

## 2. Warnings and failures

**Zero tolerance.** A clean build is the normal state, not an aspiration. No
compiler warnings, no clippy warnings, no deprecation warnings, no failing or
ignored tests. The bar applies to local work exactly as it applies to CI —
`mise run ci` is the gate, and a red gate is not something to work alongside.

This is not perfectionism. Warning output has value only while it is empty by
default: the moment three known warnings are tolerated, the fourth — the real
one — scrolls past unread. The cost of a tolerated warning is not the warning,
it is every future warning it hides.

**The only exception is genuine impossibility**, not inconvenience. Two
dependencies with incompatible requirements, a deprecated API whose replacement
is not yet released, an upstream bug with no workaround. "It's noisy to fix",
"we'll get to it", and "it's only a warning" are not exceptions.

When an exception is genuinely unavoidable:

- **Silence it as narrowly as possible.** `#[allow(...)]` on the specific item,
  never a crate-level `#![allow(...)]`, never a blanket lint disable in
  `Cargo.toml` or CI flags.
- **Justify it in place**, per §1's upstream-defect rule: what forces it, which
  dependency and version, and the upstream issue. An `#[allow]` without a
  reason is itself a defect.
- **Make it removable.** The justification exists so the next person can delete
  the suppression when the blocking condition lifts, instead of assuming it was
  always necessary.

A deprecation warning is a scheduled breakage, so treat it as work with a
deadline rather than as background noise.

## 3. Git

**Never pull. Never push.** Neither happens automatically, as a convenience, or
as the tail end of some other task. Both are explicit human actions.

**Committing is fine, including on `main`.** No branch ceremony is required for
ordinary work. Branch when there is an actual reason to isolate the work, not
as a default.

Commit messages: imperative subject, and a body that explains *why* when the
reason is not evident from the diff. Reference the architecture or vision
section a change follows from where it clarifies the intent.
