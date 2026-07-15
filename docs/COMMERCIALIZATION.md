# Keelson — Commercialization Specification

How Keelson is packaged, licensed, priced, supported, and sold into safety- and
security-critical programs. Pairs with `COMPLIANCE.md` (what must be true to certify) and
`ARCHITECTURE.md` (what gets built, when).

Core thesis: **you do not sell the multi-repo workflow — you sell reproducible, certifiable
baselines and the qualification evidence around them.** Convenience drives adoption
(open-source core). Compliance drives revenue (the commercial tiers).

---

## 1. Editions & feature matrix

Open-core. The core is genuinely useful and free, so the tool spreads bottom-up; the
compliance surface is commercial, because only regulated buyers need it and only they will
pay for it.

| Capability                                   | Core (OSS) | Team | Compliance |
|----------------------------------------------|:---------:|:----:|:----------:|
| Manifest + composition + `graph`             | ✅ | ✅ | ✅ |
| `sync` / `lock` / `switch` / `status`        | ✅ | ✅ | ✅ |
| Cross-repo changesets (`change`)             | ✅ | ✅ | ✅ |
| TUI fleet dashboard                          | ✅ | ✅ | ✅ |
| `import` from west/repo                       | ✅ | ✅ | ✅ |
| GitHub + GitLab orchestration                | ✅ | ✅ | ✅ |
| SSO / SAML, org policy, forge-enterprise      | —  | ✅ | ✅ |
| Priority support + SLA                        | —  | ✅ | ✅ |
| **SBOM export (CycloneDX + SPDX)**            | —  | ✅ | ✅ |
| **Signed lockfile / provenance attestation**  | —  | —  | ✅ |
| **Signature-verification enforcement policy** | —  | —  | ✅ |
| **`haw evidence` cert bundle**               | —  | —  | ✅ |
| **Audit log export**                          | —  | ✅ | ✅ |
| **Air-gapped deployment + offline licensing** | —  | —  | ✅ |
| **FIPS crypto build variant**                 | —  | —  | ✅ |
| **LTS with backports**                        | —  | —  | ✅ |
| **Tool Qualification Kit** (per standard/LTS) | —  | —  | ✅ (add-on) |
| Warranty + indemnification                    | —  | ltd | ✅ |

Rule of thumb: anything that is *evidence for a regulator* lives in Compliance. Anything that
is *convenience or scale* can live in Core/Team.

---

## 2. Licensing model

- **Dual OSS license for the core:** MIT OR Apache-2.0 (already chosen — good, permissive,
  trusted, avoids copyleft friction for embedded vendors).
- **Commercial license** for Team/Compliance features and the Qualification Kit. Keep the
  commercial code in a separate crate/repo so the OSS/commercial boundary is clean and
  auditable (matters for buyers who inspect what they run).
- **License enforcement:** signed license file, **offline/air-gap activation** (no phone-home
  — required for GDPR zero-egress and classified nets). Never transmit personal data during
  activation.
- **Escrow:** offer **source-code escrow** for Compliance customers (release conditions:
  vendor insolvency / EOL). Standard ask in aviation/defense procurement.

---

## 3. Version support & LTS policy

Safety programs freeze a tool version for **years** and re-qualify rarely. The support policy
*is* a product feature.

- **SemVer** for the binary; a separate, explicit **format version** for `haw.toml` and
  `haw.lock` (they are interfaces — see §4).
- **LTS branches:** designated LTS releases with a committed support window (e.g. 3–5 years),
  security backports, and a **frozen Qualification Kit** pinned to that exact version.
- **Errata channel** per LTS: known limitations, security advisories, and their fixes,
  published in a form the customer can cite in their cert data package.
- Each LTS ships **signed + reproducible** binaries (`COMPLIANCE.md §5.3`) so the customer can
  prove the binary they run is the qualified one.

---

## 4. Format stability contract

`haw.toml` and `haw.lock` are the durable interface — customers commit them for a decade.

- Versioned schema with a `schema`/format field; documented migration path forward-only.
- **Backward-compatible reads** within a major format version; breaking changes bump the
  format major and ship a `haw migrate`.
- Canonical, byte-stable serialization of the lock (prereq for signing + determinism).
- Published JSON Schemas for `--format json` outputs, versioned independently.

---

## 5. The Tool Qualification Kit (the flagship paid artifact)

This is what actually commands enterprise pricing. It is a **data package**, sold per
standard and per LTS version, mirroring how LDRA/Vector/AbsInt sell qualification support.

Contents (produced from the shared artifact set in `COMPLIANCE.md §2.6`):
- Tool Operational Requirements + requirements→test traceability matrix.
- Test suite + machine-readable results, per supported OS, run against the exact LTS binary.
- Safety/Operation Manual (intended use, operating envelope, user error-avoidance measures).
- Known-limitations & errata, versioned to the LTS.
- Configuration + build-provenance record for the qualified binary.
- Per-standard mapping + templates: **DO-330 / ISO 26262-8 / IEC 61508-3 / EN 50128 /
  ASPICE / IEC 62304**.

Delivery: versioned archive tied to one LTS binary hash; updated on errata; re-issued per new
LTS. Priced as an annual add-on **per program** (see §7).

---

## 6. Support & SLA tiers

| Tier        | Response       | Channels            | Scope                              |
|-------------|----------------|---------------------|------------------------------------|
| Community   | best-effort    | issues / forum      | Core OSS                           |
| Team        | next business  | email               | Team features, upgrades            |
| Compliance  | SLA (e.g. 4h P1)| email + named contact| LTS, backports, qualification kit, cert-audit support |

Compliance tier includes **certification-liaison support**: help answering an auditor/DER's
questions about the tool during the customer's cert campaign. High-value, low-volume, sticky.

---

## 7. Pricing model

Regulated buyers budget by **program**, not by seat. Price to how they buy.

- **Core:** free (OSS).
- **Team:** annual, per-organization or per-seat band (SSO, support, SBOM, audit log).
- **Compliance:** annual, **per program / per product line** — the air-gap, signing,
  evidence bundle, LTS, warranty. Programs have dedicated tool budget lines; land there.
- **Qualification Kit:** annual add-on **per standard × per program**, tied to an LTS.
- **Certification-liaison / services:** day-rate or retainer during a cert campaign.

Avoid pure per-seat for Compliance — it caps at the wrong number and fights procurement.
High ACV, few logos, long retention is the shape.

---

## 8. Deployment models

- **OSS binary** — Homebrew / Scoop / `cargo install` / distro packages (adoption path).
- **Self-hosted / on-prem** — enterprise forge integration, org policy, SSO.
- **Air-gapped** — vendored build, offline license, no telemetry, no egress; the reference
  deployment for classified/regulated networks. This mode is a *feature*, not an afterthought.

---

## 9. Documentation & collateral to ship (the sales/audit kit)

Non-code deliverables that gate enterprise sales as hard as features do:

- **Security Whitepaper** — SDLC, memory-safety, dependency assurance, network surface,
  crypto inventory, disclosure policy.
- **Compliance Mapping Pack** — the per-standard tables (DO-330 / ISO 26262 / IEC 61508 /
  EN 50128 / ASPICE / CRA / SSDF).
- **Privacy & Data-Protection Notice + DPA template** — GDPR posture, data-flow, DPIA input.
- **Safety/Operation Manual** — per LTS (also part of the Qualification Kit).
- **Admin/Deployment Guide** — self-hosted + air-gapped.
- **SBOM of Keelson itself** — per release, signed.
- **`security.txt`, vulnerability-disclosure policy, ECCN/export classification statement.**

---

## 10. Go-to-market sequence

Bottom-up adoption first, then monetize the regulated core.

1. **Wedge (OSS):** capture `west`/`repo` refugees (Zephyr, robotics, drones, space, RTOS).
   `import --from west.yml|default.xml` is the frictionless on-ramp. Win on Rust, no Python,
   no detached HEAD, no symlinks, a real TUI, and a committed lockfile.
2. **Team:** convert orgs that hit scale — SSO, support, SBOM, audit log. Shorter sales cycle.
3. **Compliance:** land 1 design-partner in ASPICE or DAL-C/D to co-specify the evidence +
   Qualification Kit; they validate that the moat is worth money and become the reference.
4. **Enterprise avionics/defense:** air-gap, FIPS, LTS, kit, liability. Longest cycle,
   highest ACV; enter only with a reference and a hardened kit.

Channels: direct to Methods & Tools / SCM leads at tier-1s; **partner** with existing
verification vendors (LDRA, Vector, Parasoft, AbsInt) as a complementary SCM layer; SI /
consultants running ClearCase/PTC-Integrity→Git migrations.

---

## 11. Compliance/commercial roadmap (tied to build phases)

| Phase (see ARCHITECTURE) | Ships                                   | Unlocks commercially                 |
|--------------------------|-----------------------------------------|--------------------------------------|
| 1 — Double-layer MVP     | lock, sync, drift, json output, signed releases | Core adoption; determinism baseline |
| 2 — Composition depth    | overlays, audit log, **SBOM export**    | Team tier; CRA/SBOM story            |
| 3 — MR depth             | signing, provenance, `haw evidence`, SHA-256 | Compliance tier; evidence bundle    |
| 4 — TUI actions          | interactive cockpit                     | Team/Compliance stickiness           |
| 5 — Migration/dist       | `import`, packaging                     | Wedge adoption at scale              |
| later                    | FIPS build, per-standard Qualification Kits | Enterprise avionics/defense; add-on revenue |

**Do not sell Compliance before Phase 3 exists.** Until `lock`, SBOM, signing, and the
evidence bundle are real, there is no certifiable artifact to charge for — only Core value.
