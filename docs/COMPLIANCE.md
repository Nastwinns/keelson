# Keelson — Compliance, Certification & Security Specification

Target market: safety- and security-critical programs (avionics, space, defense, rail,
automotive, industrial, medical). These buyers cannot adopt an SCM tool unless it produces
**certification evidence**, is itself **qualifiable**, has an auditable **security posture**,
and is **data-protection clean**. This document specifies what Keelson must provide, per
domain, and maps each requirement to a technical feature and a delivery phase.

Terminology: "the tool" = the `haw` binary + `keel-core`. "The applicant" = the customer's
safety/security team who owns the certification argument. Keelson supplies **evidence and a
qualification kit**; the applicant owns the final determination in their plan (PSAC, safety
plan, SEooC assumptions).

---

## 1. Where Keelson sits in the lifecycle

Keelson is a **Software Configuration Management (SCM) + composition** tool. It decides
*which revision of which repository enters a build* and records that decision immutably. It
does **not** compile, generate, or verify airborne/embedded code. That scoping is the single
most important sentence for qualification: it bounds the tool's failure modes to
"selects/records the wrong source set" — not "emits wrong object code".

Consequences of that scope:
- The tool's output (`keel.lock` + materialized tree) is **verifiable independently** by the
  downstream build + review, which lowers required qualification rigor.
- The evidence it produces (baseline, SBOM, provenance) is consumed by the customer's SCM,
  safety, and security processes — Keelson is an *evidence producer*, not an authority.

---

## 2. Tool qualification

Keelson is **qualification-relevant** in every safety standard because a config/composition
tool can select the wrong source into a safety build. The classification and rigor differ by
standard; the *artifacts* Keelson must supply are largely shared (§2.6).

### 2.1 DO-178C / DO-330 (airborne) & DO-278A (ground/ATM)
- Tool qualification governed by **DO-330**. Criterion is set by whether the tool can insert
  an error into the product and/or fail to detect one:
  - If the customer's process re-verifies the materialized tree/build independently →
    **Criterion 3** → typically **TQL-5** (lowest rigor).
  - If the lockfile/tree is trusted without independent re-verification → **Criterion 1** →
    **TQL-4** (or higher at DAL A/B).
- Keelson's recommendation to applicants: keep the downstream build+review as the verifying
  activity so the tool stays Criterion 3 / TQL-5. Document this in the PSAC.
- Deliverables Keelson supplies: **Tool Operational Requirements (TOR)**, **Tool
  Qualification Plan (TQP)**, tool requirements + test cases + results with traceability,
  **Tool Accomplishment Summary (TAS)** template, and a **known-limitations / errata** list.

### 2.2 ISO 26262-8:2018 §11 (automotive) + ASPICE
- Determine **Tool Confidence Level** TCL1–3 from Tool Impact (TI) × Tool error Detection
  (TD). Keelson is TI2 (a malfunction can violate a safety requirement) with achievable
  TD1/TD2 if the customer verifies the build → typically **TCL1–TCL2**.
- Qualification methods Keelson supports: **1a** increased confidence from use (usage
  metrics, field-history template), **1b** evaluation of the tool development process (our
  SDLC evidence, §7), **1c** validation of the tool (our test suite + results).
- ASPICE mapping: **SUP.8 Configuration Management** and **SUP.10 Change Request Management**
  (see §3). Provide the mapping table as a sales/audit artifact.

### 2.3 IEC 61508-3 (industrial functional safety)
- Offline support tool classification **T2/T3**. Keelson contributes to what is built →
  treat as **T3** unless the customer's build independently re-derives the source set.
- Supply: tool validation evidence, version + configuration record, known-defects list.

### 2.4 EN 50128 / EN 50716 (rail)
- Tool class **T2/T3** (§6.7). Supply a **Tool Qualification Report** and evidence the tool
  is used within its validated operating envelope.

### 2.5 Space (ECSS-E-ST-40 / ECSS-Q-ST-80) & medical (IEC 62304)
- Space: tool used within an SCM plan; supply configuration + validation records.
- Medical IEC 62304 §8 SOUP/configuration management: supply SBOM + baseline evidence.

### 2.6 The shared qualification artifact set (produced once, mapped per standard)
1. **Tool Operational Requirements** — what `haw` shall do, in verifiable statements.
2. **Requirements → test traceability matrix** (every requirement has a test).
3. **Test suite + machine-readable results** per released LTS, per supported OS.
4. **Safety/Operation Manual** — intended use, operating envelope, constraints, and the
   **error-avoidance measures the user must apply** (e.g. "commit the lockfile", "verify
   the build re-derives the tree").
5. **Known limitations & errata**, versioned.
6. **Configuration record** — exact tool version, dependency versions, build provenance.
7. **Per-standard mapping tables** (DO-330 / ISO 26262-8 / IEC 61508 / EN 50128 / ASPICE).

> Determinism is a hard requirement for all of the above: same inputs ⇒ byte-identical
> `keel.lock` and tree selection, on every OS. No wall-clock, no map iteration order, no
> network nondeterminism in resolution. See §8.

---

## 3. Reproducibility & SCM evidence

The commercial and certification wedge. Keelson turns "trust our process" into "here is the
signed, reproducible baseline".

- **`keel.lock` = the configuration baseline.** Every repo pinned to an exact object id.
  Committed, diff-reviewable, machine-verifiable.
- **Deterministic resolution.** Documented, versioned resolution algorithm (see
  `resolver/mod.rs`); overlay precedence is total and stable.
- **Drift detection.** `haw status` / `haw verify` proves the on-disk tree matches the
  baseline — this *is* a configuration verification activity for the customer's SCM plan.
- **Evidence bundle.** `haw evidence` emits: baseline (lock) + SBOM + provenance
  attestation + tool configuration record, as one signed archive for the cert data package.

Standards mapping:

| Requirement                         | DO-178C        | ISO 26262      | ASPICE  |
|-------------------------------------|----------------|----------------|---------|
| Baselines / configuration ids       | SCM (Table A-8)| Part 8 §7      | SUP.8   |
| Change control across repos         | SCM            | Part 8 §8      | SUP.10  |
| Reproducible build inputs           | SCM / SC       | Part 8 §7      | SUP.8   |
| Traceability of what shipped        | SCM / verify   | Part 8 §7–8    | SUP.8/10|

---

## 4. Supply-chain security & SBOM

Regulatory tailwind — increasingly mandatory, not optional.

- **SBOM export** in both dominant formats: **CycloneDX** and **SPDX 2.3 (ISO/IEC 5962)**,
  SPDX 3.0 when stable. Include NTIA minimum elements (supplier, component, version, unique
  id, dependency relationship, author, timestamp).
- Keelson emits an SBOM **of the composed product** (repos + their pinned ids) *and* ships
  an SBOM **of `haw` itself** (its Rust dependency tree) per release.
- **EU Cyber Resilience Act (Regulation (EU) 2024/2847).** In force since Dec 2024; core
  obligations apply ~Dec 2027, vulnerability/incident reporting ~Sept 2026. Requires
  manufacturers of products-with-digital-elements to maintain an SBOM and handle
  vulnerabilities. Keelson's lock→SBOM path is a direct compliance enabler for customers,
  and Keelson itself must be CRA-conformant as a product we sell.
- **US EO 14028 / NIST SSDF (SP 800-218)** and **SLSA v1.0 provenance.** Keelson emits
  build/composition **provenance attestations** (in-toto style) so the composed tree carries
  verifiable "what went in, from where, at which id".

---

## 5. Cryptography & integrity

Sensitive customers require cryptographic integrity end to end, and a documented crypto
inventory. Design principles: verify by default where the customer opts in, never invent
crypto, be FIPS-swappable, be secret-hygienic.

### 5.1 Signature verification (source integrity)
- Verify **commit/tag signatures** before checkout when the customer enables it: OpenPGP
  (GPG), **SSH signing**, and **Sigstore / gitsign** (keyless). Policy modes:
  `off | warn | require`. `require` fails `sync` on any unsigned/unverified repo.
- Trust anchors configured per workspace (allowed signer sets, keyrings, Fulcio roots).

### 5.2 Baseline integrity
- **Signed lockfile / signed resolution attestation.** `haw lock --sign` produces a
  detached signature or an in-toto attestation over the canonical lock bytes, so a reviewer
  can prove the baseline was not altered after approval.
- Canonical, byte-stable lock serialization is a prerequisite (§8).

### 5.3 Tool identity (a qualified tool must be identity-verifiable)
- **Signed, reproducible `haw` releases.** Cosign/Sigstore signatures + SHA-256 checksums +
  SLSA build provenance for every artifact and every LTS. Customers verify the binary they
  run matches the qualified one.

### 5.4 Hash agility
- Support git's **SHA-256 object format** alongside SHA-1, and record which is in use in the
  lock. Long-lived safety programs outlive SHA-1's collision safety margin; store the
  strongest available id. Never rely on SHA-1 as the sole integrity guarantee for a baseline.

### 5.5 Crypto module & transport
- TLS to forge APIs via **rustls**; no protocol downgrade, pinned minimum TLS 1.2 (prefer
  1.3). For **FIPS 140-3** environments, ship a build backed by a FIPS-validated module
  (e.g. rustls + aws-lc-rs FIPS, or system OpenSSL FIPS) and document the boundary.
- Publish a **cryptographic inventory** (algorithms, libraries, versions) — required for
  crypto-agility audits and FIPS/Common-Criteria conversations.

### 5.6 Secret handling
- Forge tokens and credentials are **never** written to `keel.toml`, `keel.lock`, logs, or
  workspace state. Source them from the OS keychain, `git credential` helpers, or a secrets
  manager (Vault) via env/helper. **Redact** any credential-shaped string in logs and errors.

### 5.7 Export control (Keelson as a crypto-bearing product)
- Keelson uses/links cryptography → likely **ECCN 5D002** under the US EAR. Path:
  self-classify as mass-market (740.17 / mass-market note) and, for the open-source core,
  file the **published-encryption-source notification** to BIS/NSA (§742.15(b)). Publish the
  ECCN and classification so customers can clear import/use in their jurisdiction.
- Wassenaar/dual-use: dual-use commercial software; keep it out of ITAR scope by not
  bundling controlled technical data. Defense customers own their ITAR/EAR program controls.

---

## 6. Data protection (GDPR & equivalents)

Keelson processes personal data incidentally: **git author/committer name + email, forge
usernames, PR/MR reviewer identities, CI actor, review timestamps**. All are personal data
under GDPR (and CCPA, LGPD, UK-GDPR).

Design stance — this is a *selling point*, not just a constraint:

- **Local-first, zero-egress by default.** For local operation, data never leaves the
  operator's machine/network → the **customer is the controller** and Keelson's vendor is
  **not a processor** (nothing is transmitted to us). This is the cleanest possible posture
  for classified/regulated networks.
- **No telemetry by default.** Any future telemetry is strictly opt-in, anonymized, EU-
  hostable, and fully documented; air-gapped builds have **no network path** to us at all.
- **Offline license activation.** License validation must not phone home with personal data;
  provide file-based/offline activation for air-gapped and sovereign environments.
- **Data residency / sovereignty.** Self-hosted and air-gapped deployments keep all data
  inside the customer boundary. For any optional hosted component (license server, mirror),
  offer an **EU region** and a signed **Data Processing Agreement (DPA)**.
- **Records of processing (Art. 30) & DPIA support.** Provide a data-flow description and a
  DPIA-input template documenting exactly which personal-data fields Keelson reads and where
  they go (answer: nowhere, for local use).
- **Right to erasure vs immutable history.** Keelson creates **no new personal-data store**;
  it reads git/forge metadata that already exists. Erasure requests are handled at the git
  history / forge layer by the customer — Keelson documents this boundary rather than
  pretending to satisfy erasure over immutable commit objects.
- Ship a **privacy notice** and a `security.txt` / `.well-known/security.txt` contact.

---

## 7. Security of the tool itself (secure SDLC)

A tool sold into security-critical programs is itself part of the attack surface. Evidence
of a secure SDLC is a purchase precondition and feeds ISO 26262 method 1b / DO-330 tool
development assurance.

- **Memory safety.** `unsafe_code = "forbid"` workspace-wide (already set). A Rust,
  memory-safe SCM tool is a concrete differentiator vs C/Python incumbents — state it.
- **Dependency assurance.** `cargo-audit` (RUSTSEC advisories), `cargo-deny` (license policy
  + advisory + banned-crate gates), and supply-chain review (`cargo-vet`/`cargo-crev`) in CI,
  failing the build on violations.
- **Vendored + reproducible builds.** Vendor dependencies for air-gapped rebuild; pin the
  toolchain (`rust-toolchain.toml`); byte-reproducible release builds.
- **Own SBOM published per release** (§4) with signatures (§5.3).
- **Vulnerability disclosure & CVE handling.** Published policy, security contact,
  coordinated-disclosure window, CVE issuance, and per-LTS backport commitment (§ commercial).
- **Bounded, documented network surface.** The tool performs network I/O *only* for explicit
  git/forge operations the user invoked; document every egress. No hidden calls.
- **Least privilege.** No elevation; no writing outside the workspace + configured cache dir.

---

## 8. Auditability & determinism (cross-cutting)

- **Determinism contract.** Same manifest + lock + overlays ⇒ byte-identical resolution and
  tree selection on Linux/macOS/Windows. No `Date.now`, no unordered iteration in
  serialization (use ordered maps — already `IndexMap`), canonical TOML emission for the lock.
- **Structured audit log.** Every mutating operation records actor, operation, affected
  repo, before/after object id, timestamp — machine-readable (JSON) for CI evidence capture.
- **Machine-readable output.** `--format json` on status/verify/graph/evidence so pipelines
  can capture and diff cert evidence automatically. Stable, versioned schemas.
- **Stable exit codes** so CI gates are reliable (0 ok, distinct non-zero for drift / verify
  failure / signature failure).

---

## 9. Feature backlog mapped to compliance requirements

What must be *built* to make the above real. Phases refer to `ARCHITECTURE.md §6`.

| Feature                                        | Enables                                  | Phase |
|------------------------------------------------|------------------------------------------|-------|
| `keel.lock` + deterministic resolution         | Baseline evidence, qualification         | 1     |
| Drift detection (`status`/`verify`)            | Config verification activity             | 1     |
| `--format json` + stable schemas + exit codes  | Auditability, CI evidence capture        | 1     |
| Canonical/byte-stable lock serialization       | Signed baseline, determinism             | 1     |
| Structured audit log                           | Auditability                             | 2     |
| SBOM export (CycloneDX + SPDX)                  | CRA, EO 14028, IEC 62304 SOUP            | 2     |
| Commit/tag signature verification (gpg/ssh/sigstore) | Source integrity                    | 2/3   |
| Signed lockfile / in-toto attestation          | Baseline integrity                       | 3     |
| Provenance attestation (SLSA/in-toto)          | Supply-chain assurance                   | 3     |
| `haw evidence` bundle                         | One-shot cert data package               | 3     |
| SHA-256 object-format support + record         | Hash agility, long-lived programs        | 3     |
| Signed + reproducible `haw` releases          | Tool identity, qualification             | 1→ongoing |
| Vendored deps + reproducible tool build        | Air-gap, secure SDLC                     | 1→ongoing |
| FIPS-validated crypto build variant            | FIPS 140-3 environments                  | later |
| Offline license activation                     | Air-gap, GDPR zero-egress                | commercial |
| Cryptographic inventory doc                    | Crypto-agility / FIPS / CC audits        | doc   |
| Qualification Kit per LTS                       | DO-330 / ISO 26262 / IEC 61508 / EN 50128| commercial |

See `COMMERCIALIZATION.md` for how these are packaged, licensed, and supported.
