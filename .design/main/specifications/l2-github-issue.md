# GitHub Issue Reporting

**Version:** 1.0.2
**Status:** Stable
**Layer:** implementation
**Implements:** l1-error-reporting.md

## Overview

The concrete error reporter: on an unrepairable error, with user consent, it sanitizes diagnostics, de-duplicates against existing issues, and files or updates a GitHub issue.

## Related Specifications

- [l1-error-reporting.md](l1-error-reporting.md) - The model this implements.
- [l2-security.md](l2-security.md) - Egress gate and redaction the reporter uses.
- [l2-cli.md](l2-cli.md) - Command grammar standard.

## 1. Motivation

The model needs a concrete tracker integration with consent, scrubbing, and dedup so real failures become actionable issues without spam or leaks.

## 2. Constraints & Assumptions

- Target repository and enablement are configured (`<state>/config.json`).
- Filing passes the security egress gate (consent + audit).
- Dedup searches existing issues by an error signature.

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| ERR-1 Consent-gated | Filing requires a consent prompt or a remembered always/never preference; passes the egress gate. In `ask` mode the user sees the exact payload, and what is filed is byte-for-byte what was shown (CB-2). |
| ERR-2 Privacy-scrubbed | A sanitizer strips secrets/user content; only allowlisted diagnostic fields are sent. Free-form text that rides along — the error message, a pre-filled note about prior occurrences — passes the same redaction (`l2-security` §4.12), with paths reduced to tier-relative form and workspace and file names removed. |
| ERR-3 De-duplicated | The fault fingerprint (§4.3) is searched; matches are updated, not duplicated. The fingerprint is version-independent on purpose, so a regression finds its existing report (ERR-7); the version travels with each occurrence as an attribute. |
| ERR-4 Actionable | The issue includes app version, sanitized stack, environment summary, and repro hints. |
| ERR-5 Local-first | The error is written to the workspace logs regardless of filing. |
| ERR-6 Submission autonomy modes | `never` / `ask` / `always` realize off / confirm / automatic; the default is `ask` (confirm). The mode is a standing grant on the human-write-only authority plane: it is set only through `cronus report consent` or the settings surface, never by editing a file the agent can write, and every automatically filed report remains visible locally. |
| ERR-7 Fault identity and lifecycle | **Partial.** One report per fingerprint, updated on recurrence (§4.3). Reopening a report as a **regression** when a fault recurs at or after the version its fix was claimed in, and gating re-filing on the fault's substatus (new / regressed rather than merely ongoing), are pending. |
| ERR-8 Refusals name their reasons | A report the pipeline declines to file — consent `never`, egress not authorized, redaction unable to make the payload safe — says which of those conditions stopped it, never only that it was not sent. |
| ERR-9 Diagnostics reach the triggering surface | **Pending.** A report or refusal raised from a surface with no diagnostic channel (a keystroke, an automation fire) is to degrade into an interactive affordance that can explain itself, rather than fail into a log nobody reads. |

## 4. Detailed Design

### 4.1 Pipeline

```mermaid
graph TD
    ERR[error] --> LOGL[log locally]
    LOGL --> C{consent: always/ask/never}
    C -->|never| END[stop]
    C -->|ask -> yes / always| SAN[sanitize]
    SAN --> SIG[compute signature]
    SIG --> SRCH{existing issue?}
    SRCH -->|yes| UPD[comment/bump issue]
    SRCH -->|no| NEW[create issue]
```

Filing uses the GitHub CLI/API. Config: `report.enabled`, `report.repo`, `report.consent` (always|ask|never; default `ask`, per ERR-6 — the default mode is confirm).

### 4.2 Command surface

| Action | CLI | TUI | Library (no code) |
| --- | --- | --- | --- |
| report last error | `cronus report` | `/report` | `reporter.report() -> IssueRef` |
| set consent | `cronus report consent <always\|ask\|never>` | `/report consent …` | `reporter.setConsent(mode) -> void` |

### 4.3 Error fingerprinting

Before filing or updating a GitHub issue, the error is **fingerprinted**: a normalized canonical representation of the error text is hashed (BLAKE3) and checked against a persistent dedup table. Matching fingerprints across episodes surface "you have seen this exact error N times before" — like `git blame` for bugs.

#### Normalization

```text
[REFERENCE]
normalize_message(msg: &str) -> String:
  // 1. Replace hex addresses with a sentinel so 0xdeadbeef ≡ 0xcafebabe
  stripped = regex r"0x[0-9a-fA-F]+" .replace_all(msg, "0xADDR")
  // 2. Replace the user's home directory with /USER so cross-machine panics match
  return stripped.replace(home_dir(), "/USER")

fingerprint_error(error_type: &str, message: &str) -> String:
  canonical = format!("{error_type}|{normalized_message}")
  return blake3::hash(canonical.as_bytes()).to_hex()  // 64 hex chars
```

Normalization strips machine-specific and address-layout-specific noise, so the same panic on two different machines — or at different ASLR addresses — produces the same 64-character fingerprint.

#### Dedup table

```sql
-- [REFERENCE] illustrative, not final DDL
CREATE TABLE error_fingerprints (
    hash             TEXT NOT NULL,
    episode_id       TEXT NOT NULL,
    occurrence_count INTEGER NOT NULL DEFAULT 1,
    first_seen       INTEGER NOT NULL,
    last_seen        INTEGER NOT NULL,
    PRIMARY KEY (hash, episode_id)
);
CREATE INDEX error_fp_hash ON error_fingerprints(hash, last_seen DESC);
```

Each `(hash, episode_id)` pair tracks how many times a given fingerprint appeared within a specific episode. When a capture records an error, the pipeline:

1. Computes `fingerprint_error(error_type, message)`.
2. Upserts: increments `occurrence_count` and refreshes `last_seen` if the row exists; inserts with `count=1` otherwise.
3. Queries prior episodes (excluding the current) where this hash appeared — returns the episode IDs, occurrence counts, and last-seen timestamps.
4. If prior matches exist, surfaces "seen N times before in episodes \[IDs\]" in the capture response and pre-fills the GitHub issue with prior resolutions — after the same redaction as every other field (ERR-2), since a resolution note written locally may carry user content.

#### Lookup API

```text
[REFERENCE]
record(hash: &str, episode_id: &str) -> occurrence_count: i64
matches(hash: &str, exclude_episode_id: &str) -> Vec<FingerprintMatch>
total_occurrences(hash: &str) -> i64
```

`matches` orders results by `last_seen DESC` and caps at 10 prior episodes. `total_occurrences` returns the sum across all episodes for "seen N times total" messages.

## 5. Drawbacks & Alternatives

- **Signature collisions:** distinct errors may normalize alike; mitigated by including the error type in the fingerprint. The version is deliberately kept out of it, so a regression lands on its existing report (ERR-7).
- **Alternative — email/other tracker:** GitHub default; the reporter is adaptable to other trackers later.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[ERR]` | `.design/main/specifications/l1-error-reporting.md` | Invariants this implements |
| `[SECURITY]` | `.design/main/specifications/l2-security.md` | Egress gate + redaction |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.2 | 2026-09-23 | Core Team | Consistency pass (2026-09-23): ERR-6…ERR-9 were unmapped — ERR-6 resolves the default-consent TBD to `ask` (confirm) with the mode held on the authority plane; ERR-7 Partial (regression reopening pending); ERR-8 refusals name their causes; ERR-9 Pending. The ERR-3 row keyed dedup on a signature including the version while §4.3 fingerprints without it — the fingerprint is deliberately version-independent so regressions find their report. Free-form text (messages, pre-filled prior resolutions) now passes the same redaction, and a confirmed payload is filed byte-for-byte as shown (CB-2). |
| 1.0.1 | — | Core Team | Last version before this section was added; earlier revisions are recorded in version control. |
