# Verifiable Attestation

**Version:** 1.0.1
**Status:** Stable
**Layer:** concept

## Overview

An autonomous system that imports, exports, replicates, and releases artifacts needs
to answer a question no amount of good behavior can answer on its own: *is this
artifact really what it claims to be, and did who I think produce it?* An imported
skill, an exported automation bundle, a synced device's payload, a released office —
each is a unit of trust that arrives from *somewhere*, and "it's in our registry" or
"it came over TLS" proves neither its integrity nor its authorship.

This spec defines **verifiable attestation**: a produced or distributed artifact
carries a signed **witness** that binds (a) its exact content set, (b) the identity
that produced it, and (c) a set of typed claims about it (built-from-this-source,
passed-these-gates, produced-by-this-role). Any party can **verify the witness
offline**, using only public information, without trusting the producer, the channel,
or a central authority. **Trust is established by verifying the witness, not by
trusting how the artifact arrived.** An artifact that should carry a witness and
doesn't — or whose witness fails — is default-denied.

It is the *supply-chain / integrity* trust layer. It is distinct from and
complementary to `l1-context-provenance` (which governs untrusted *content* entering a
prompt) and `l1-operational-ledger` (which records the *data-provenance* of the
agent's own actions): attestation is about the **authenticity and integrity of an
artifact**, checkable by a third party.

## Related Specifications

- [l1-security.md](l1-security.md) — SEC-1 secret isolation governs the signing key (AT-8); attestation is the integrity counterpart to secret-at-rest protection.
- [l1-extensions.md](l1-extensions.md) — EXT-11 verifies an imported extension's witness before activation; sharpens EXT-8 declared provenance into proven provenance.
- [l1-acp.md](l1-acp.md) — trust levels (trusted/restricted/anonymous) are elevated by a witness handshake before the first privileged cross-office interaction (AT-5).
- [l1-multi-device-sync.md](l1-multi-device-sync.md) — SY-9 authenticated device membership is realized by attestation; a replicated payload carries a witness (AT-2).
- [l1-version-control.md](l1-version-control.md) — a release attests its author (VC-1 authority) and passed gates (AT-4).
- [l1-quality-standards.md](l1-quality-standards.md) — passed quality gates are attestable typed claims (AT-4).
- [l1-context-provenance.md](l1-context-provenance.md) — the complementary content-trust boundary (composition safety vs artifact integrity).
- [l1-nodus-portability.md](../../nodus/specifications/l1-nodus-portability.md) — the nodus realization: LP-9 extraction-bundle witness.
- [l1-reproduction-recipe.md](l1-reproduction-recipe.md) — [ADDED v1.0.1] the complementary claim: a witness proves an artifact is genuine, a recipe states how to make it again; RR-10 forbids substituting one for the other.

## 1. Motivation

cronus already has the *places* trust is needed but grants it on assertion:

- An **imported extension** (skill / plugin / connector) declares its origin
  (`l1-extensions` EXT-8) — but a declaration is a self-report. A tampered or
  impersonated extension declares whatever it likes.
- **Cross-office federation** (`l1-acp`, `l1-global-orchestration`) assigns trust by
  auth identity — but nothing proves the *artifact* exchanged is intact and from the
  claimed author before a privileged call.
- **Multi-device replication** (`l1-multi-device-sync` SY-9) authenticates membership
  — but "authenticated" needs a mechanism, and a replicated payload needs integrity.
- A **release** (`l1-version-control`) is trusted because it is in the repo — not
  because it *proves* it was produced by an authorized role and passed its gates.
- An **exported automation bundle** (`l1-automation-pipeline` AP-11) crosses machines
  with credential rebinding — but no integrity or authorship proof travels with it.

Each is a supply-chain trust boundary with no proof. The fix is one mechanism: a
signed witness that is *independently verifiable*. Then an import is trusted because
its witness verifies; a peer is trusted because its handshake verifies; a release
attests its own gates; and a tampered or impersonated artifact fails verification and
is denied — fail-closed, before it can act. This spec raises attestation from a
per-surface afterthought to one L1 contract every trust boundary uses.

## 2. Constraints & Assumptions

- Verification is **offline and pure**: given the artifact, its witness, and public
  key material, verification is a deterministic check requiring no network, no
  producer, and no central authority.
- The cryptographic mechanism (signature scheme, hash) is an implementation choice;
  this spec constrains the *properties* — binding, independence, revocation, key
  hygiene — not the algorithm.
- Attestation proves integrity and authorship; it does **not** prove the artifact is
  *good* (that is `l1-quality-standards` / `l1-evaluation-suites`) nor that its
  *content* is safe to read as instructions (that is `l1-context-provenance`).
- A conservative default is always safe: where integrity matters, treating an
  unverifiable artifact as untrusted forgoes convenience but never grants
  unearned trust.

## 3. Core Invariants

Rules every Layer 2 implementation MUST NOT violate:

- **AT-1 Independently verifiable, no central authority**: an attestation is
  verifiable by any party using only public information (published key material / a
  public verifier), without trusting the producer, the transport, or a central
  registry. Verification is a pure, offline, deterministic check.

- **AT-2 Content-set binding**: a witness binds the *exact* content it attests — a
  signed manifest enumerating the artifact's constituent items and their digests. Any
  change to the content (adding, removing, or modifying a single byte of any item)
  invalidates the witness. A witness that does not cover the whole content set does
  not attest the artifact.

- **AT-3 Authorship binding**: a witness cryptographically identifies the producer —
  the key that signed it — so a verifier learns *who* attested, not merely *that*
  something was attested. An unsigned or anonymously-signed manifest is a checksum,
  not an attestation.

- **AT-4 Explicit, typed, checkable claims**: beyond content and authorship, a witness
  MAY carry a closed set of typed claims — e.g. *built-from-source X*, *passed-gates
  G*, *produced-by-role R at autonomy L*. Each claim is independently checkable
  against evidence; a claim with no check attached is a comment, not an attestation,
  and MUST NOT be relied upon as one.

- **AT-5 Trust by verification, not by channel**: a peer or artifact is trusted for a
  privileged interaction only after its witness verifies. Trust inferred from the
  transport (it arrived over an encrypted link, it is already in our registry) is
  insufficient. Federation and import handshakes verify the witness *before* the first
  privileged interaction, not after.

- **AT-6 Default-deny the unattested**: where integrity matters, an artifact that
  should carry a witness but does not — or whose witness fails verification — is
  treated as untrusted and denied the capabilities a verified artifact would receive
  (composing `l1-extensions` EXT-3 default-deny and the `l1-acp` anonymous/restricted
  tiers). Missing or failed attestation is fail-closed, never fail-open.

- **AT-7 Append-only, revocation-by-supersession**: a witness is immutable once
  issued. Correcting or revoking an attestation is done by issuing a *superseding*
  witness — a revocation is itself an attested, signed statement — never by editing or
  silently deleting the original. The chain of supersession is auditable, so a
  verifier can detect a revoked attestation.

- **AT-8 Mechanism-agnostic, key-hygiene-bound**: the signature and digest algorithms
  are an implementation choice, but the signing key obeys the secret-isolation rules
  (`l1-security` SEC-1): it is never written to tracked files or logs, is kept
  on-device, and its compromise invalidates every attestation that depends on it.
  Rotating a key is an attested event (AT-7).

- **AT-9 Verification observability**: every verification — pass or fail, which key,
  which claims, which artifact — is recorded, so a trust decision is auditable after
  the fact and a verification-failure incident (a tampered import, an unexpected
  signer, a rotated-then-reused key) is detectable from telemetry, not only from a
  breach.

> L2 specs cannot reach RFC status until all invariants here are addressed in their "Invariant Compliance" section.

## 4. Detailed Design

### 4.1 The Witness

```text
[REFERENCE]
Witness := {
  artifact   : ArtifactId + version                         // what this attests (AT-2)
  entries    : [ { path/name, digest } ]                     // exact content set (AT-2)
  producer   : public key / key id                           // who signed (AT-3)
  claims     : [ TypedClaim ]                                 // built-from / passed-gates / … (AT-4)
  supersedes : WitnessId?                                     // revocation / correction chain (AT-7)
  signature  : sign(private_key, hash(artifact ∥ entries ∥ producer ∥ claims ∥ supersedes))
}
```

The signature covers *everything* — content set, author, claims, supersession — so
none can be altered without breaking verification.

### 4.2 Verification (pure, offline)

```text
[REFERENCE]
verify(artifact, witness, trusted_keys):
    if witness absent:                       return UNATTESTED         // AT-6 default-deny
    if witness.producer ∉ trusted_keys:      return UNKNOWN_SIGNER     // AT-3/AT-5
    if not signature_valid(witness):         return TAMPERED           // AT-2/AT-3
    for e in witness.entries:
        if digest(artifact[e.name]) != e.digest:  return TAMPERED      // AT-2
    if superseded(witness):                  return REVOKED            // AT-7
    record(verification_result)                                        // AT-9
    return VALID
```

No network, no producer, no authority — only the artifact, the witness, and the set of
keys the verifier already trusts.

### 4.3 Typed Claims (AT-4)

A witness's claims are the *checkable* assertions a consumer cares about:

| Claim | Checked against |
| --- | --- |
| `built-from-source: <commit>` | the source digest matches the named commit |
| `passed-gates: [T1,T2,T3]` | a signed gate-result the same producer attests (`l1-quality-standards`) |
| `produced-by: <role> @ <autonomy>` | the role authority model (`l1-version-control` VC-1) |

An unchecked claim is inert (AT-4): a witness may *say* "passed all gates", but a
verifier trusts it only to the extent it can check it.

### 4.4 Trust Handshake and Federation (AT-5)

```text
[REFERENCE]
federate(office_a, office_b):
    exchange witnesses
    each verifies the other's witness (4.2) BEFORE any privileged call
    VALID    → elevate ACP trust level (restricted → trusted)
    non-VALID → stay anonymous/restricted (l1-acp §4.4); no privileged interaction
```

This is the mechanism behind `l1-multi-device-sync` SY-9 (a device proves membership
by a verifying witness) and a sharper `l1-acp` trust assignment (identity + artifact
integrity, not identity alone).

### 4.5 Default-Deny and Revocation

An unverifiable artifact never receives the grants a verified one would (AT-6) — the
same fail-closed posture as extension default-deny. Revocation is not deletion: a
producer issues a *superseding* witness marking the prior one revoked (AT-7), so a
verifier holding a cached artifact still learns it is no longer attested.

### 4.6 Three Trust Boundaries, Three Owners

| Boundary | Question | Owner |
| --- | --- | --- |
| Untrusted content *entering* a prompt | can this be read as instructions? | `l1-context-provenance` |
| An artifact's *integrity + authorship* | is this really what/whose it claims? | **this spec** |
| The agent's own *action data-provenance* | where did this fact/decision come from? | `l1-operational-ledger` |

Attestation is the supply-chain floor: even a perfectly-composed, well-grounded run is
compromised if it was built from a tampered, unattested artifact.

## 5. Implementation Notes

1. Make "no witness where one is required" a hard denial at the import/activation/
   federation boundary, not a warning (AT-6).
2. Keep the trusted-key set explicit and small; verifying against an unbounded key set
   is trusting everyone.
3. Attach claims only when they are checkable; prefer fewer proven claims to many
   asserted ones (AT-4).
4. Treat key rotation as an attested event and keep the supersession chain queryable
   (AT-7) so a cached artifact's revocation is discoverable offline.

## 6. Drawbacks & Alternatives

- **Key management burden.** Signing keys must be generated, protected, rotated, and
  distributed. Justified: the boundaries it guards (imported code, federated peers,
  replicated state, releases) are the highest-consequence trust decisions the system
  makes, and AT-8 binds key hygiene to the existing secret-isolation rules.
- **Attestation ≠ quality.** A verified artifact can still be bad. Accepted by design:
  attestation proves *integrity and authorship*; `l1-quality-standards` /
  `l1-evaluation-suites` prove *goodness*. The two compose (a passed-gates claim is
  attested, AT-4).
- **Alternative — trust the registry/channel.** Rejected (AT-5): a central registry is
  a single point of trust and compromise; channel encryption proves confidentiality in
  transit, not artifact integrity or authorship.
- **Alternative — keep it an L2 extension-registry detail.** Rejected: the same
  witness mechanism is needed at import, federation, replication, and release — four
  boundaries — so it is a cross-cutting contract, not one registry's private feature.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[SECURITY]` | `.design/main/specifications/l1-security.md` | Secret-isolation rules the signing key obeys (AT-8). |
| `[EXTENSIONS]` | `.design/main/specifications/l1-extensions.md` | Import-verification boundary (EXT-11) that consumes this contract. |
| `[SYNC]` | `.design/main/specifications/l1-multi-device-sync.md` | Authenticated membership (SY-9) realized by attestation. |
| `[VERSION-CTRL]` | `.design/main/specifications/l1-version-control.md` | Release authorship + passed-gates as attested claims (AT-4). |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.0 | 2026-07-02 | Core Team | Initial spec — verifiable attestation: an artifact carries a signed witness independently verifiable offline with no central authority (AT-1); exact content-set binding, one byte invalidates (AT-2); authorship binding (AT-3); explicit typed checkable claims — built-from/passed-gates/produced-by (AT-4); trust by verification not by channel, verify before first privileged interaction (AT-5); default-deny the unattested, fail-closed (AT-6); append-only revocation-by-supersession (AT-7); mechanism-agnostic but key-hygiene-bound to SEC-1 (AT-8); verification observability for audit + tamper detection (AT-9). The supply-chain/integrity trust layer — distinct from l1-context-provenance (content-trust) and l1-operational-ledger (data-provenance); composes with l1-extensions/l1-acp/l1-multi-device-sync/l1-version-control/l1-quality-standards; nodus realization = l1-nodus-portability LP-9. |
| 1.0.1 | 2026-07-23 | Core Team | Related Specifications gains `l1-reproduction-recipe` — the complementary claim: a witness proves an artifact is genuine, a recipe states how to make it again; RR-10 forbids substituting one for the other. No invariant changed. (Row reconstructed from version control in the 2026-09-24 consistency pass.) |
