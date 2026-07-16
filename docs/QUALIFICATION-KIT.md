# Qualification kit (skeleton)

The kit a **customer references in their own** ISO 26262 / ASPICE / DO-178C dossier
to qualify `haw` as a tool in their project. haw is **not** "certified"; the
customer qualifies it per project, using the artifacts below. This document is the
skeleton — each section links to the evidence haw already produces.

> Scope statement (load-bearing — keep it narrow): **haw decides which revision of
> which repository enters a build, and reports drift. It does not compile,
> generate, transform, or verify source code.** This bounds haw's failure modes and
> keeps its tool-confidence / tool-qualification level low.

## 1. Tool classification

### 1.1 ISO 26262-8 §11 — Tool Confidence Level (TCL)
- **Tool Impact (TI):** could a haw malfunction introduce/fail-to-detect an error
  in the safety-related item? haw selects revisions and flags drift — a wrong
  selection *could* feed the wrong source into a build → **TI2** (candidate).
- **Tool error Detection (TD):** is such an error caught by other means? Yes —
  `haw verify` (exit 3 on drift), the committed `haw.lock`, code review, and the
  downstream build/test all detect a wrong revision → **TD1** (high detection).
- **Result:** TI2 × TD1 → **TCL1** (lowest). Argument: haw's output (the checked-out
  tree) is fully verified downstream by `verify` + build + review.
- *To confirm per customer project; provide the TCL determination worksheet.*

### 1.2 DO-330 / DO-178C — Tool Qualification Level (TQL)
- Three questions: (1) can the tool insert an error? (2) is its output unverified?
  (3) does its output meet/replace a DO-178C objective? haw's output (revision
  selection) is **verified** by `haw verify` + downstream build/test → question (2)
  is "No" → **qualification typically not required**, or **TQL-5** if credit is
  claimed. *Determined per project, stated in the PSAC.*

## 2. Tool Operational Requirements (TOR) — outline

The TOR is the heart of the kit: what haw does, its environment, and its expected
responses including abnormal conditions. One requirement per verifiable behavior.

| TOR ID | Requirement (what haw shall do) | Verified by |
|--------|----------------------------------|-------------|
| TOR-SYNC-1 | Materialize each repo at the exact SHA recorded in `haw.lock` | golden test + `haw verify` |
| TOR-LOCK-1 | `haw.lock` is byte-identical for identical inputs, LF-only, cross-OS | determinism test (CI matrix) |
| TOR-VERIFY-1 | `haw verify` exits 3 when any repo diverges from `haw.lock` | golden test (dirty + drift) |
| TOR-VERIFY-2 | `haw verify` exits 0 when the tree matches `haw.lock` | golden test |
| TOR-BUILD-1 | `haw build` runs each repo's declared `build =` command; reports per-repo status | golden test |
| TOR-STATUS-1 | `haw status --format json` emits the `haw.status/1` schema | schema test |
| TOR-EVID-1 | `haw evidence` bundles manifest + lock + audit + status + tool record | golden test |
| TOR-ABN-1 | Missing repo / unreachable remote / dirty tree are reported, never silently ignored | error-path tests |

*Full TOR document expands each row with rationale, inputs, outputs, and abnormal
responses. Populate from the shipped behavior + tests.*

## 3. Tool Verification (TVR) — evidence already produced

The technical backbone exists today — the kit references it:
- **Determinism / reproducibility:** `haw.lock` byte-identical across the
  Linux/macOS/Windows CI matrix (the reproducibility argument standards demand).
- **Golden CLI-output tests** (`crates/hawser/tests/golden.rs`) — snapshot
  `tree`/`status`/`sync`, the `--format json` schema, and the `verify` exit-3 gate.
- **Unit + integration tests** — 80+ across the workspace, `unsafe` forbidden,
  `clippy -D warnings` clean (see [COMPLIANCE.md](COMPLIANCE.md) §7).
- **Test → TOR trace:** each TOR row above names its verifying test (the
  requirements→test matrix *for the tool itself*, per the qualification kit).

## 4. Tool configuration index

- **Version:** `haw --version`; the release is tagged and reproducible.
- **Own SBOM:** published per release (roadmap Phase A) — CycloneDX + SPDX.
- **Signed release:** cosign/sigstore signature + SHA-256 (roadmap Phase B).
- **Crypto inventory:** algorithms/libraries/versions (see COMPLIANCE.md §5).
- **Toolchain:** Rust version pinned (`rust-version` in `Cargo.toml`), captured in
  the `haw evidence` `tool.json`.

## 5. Deliverable structure (what ships to the customer)

```
qualification-kit/
├── TOR.pdf                 tool operational requirements (§2, expanded)
├── TCL-TQL-determination.pdf   classification worksheet (§1)
├── TVR.pdf                 verification results, test → TOR trace (§3)
├── tool-config-index.pdf   version, SBOM, signature, crypto inventory (§4)
├── safety-manual.pdf       assumptions of use, constraints, known limitations
└── evidence/               machine artifacts (haw evidence bundle, SBOM, sig)
```

## 6. Packaging & pricing (business)

Per [COMMERCIALIZATION.md](COMMERCIALIZATION.md) §5: the kit is sold **per standard
× per program**, tied to an **LTS hash** so the qualified version is frozen.
Qualification is executed with a **design partner on a real project** (per-project
by nature), then reused/adapted for subsequent programs.

## 7. What to build to complete the kit

1. Expand the **TOR** (§2) into a full document from the shipped behavior + tests.
2. Add the **requirements→test trace** for the tool (mostly present — formalize it).
3. Ship the **own SBOM + signed release** (roadmap Phases A/B) — the integrity
   evidence auditors ask for.
4. Draft the **safety manual** (assumptions of use, e.g. "build out of tree so
   `verify` is not confused by build artifacts", "protect `main`, no force-push").
5. Qualify against **one design partner** project; capture the assessor's findings.

> Reminder: keeping haw narrow (no in-tool code verification / traceability) is what
> keeps this kit small and cheap — the deferral of `haw-trace` is a qualification
> decision, not only a scope one. See [PROD-VALIDATION.md](PROD-VALIDATION.md).
