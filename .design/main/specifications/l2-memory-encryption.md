# Memory Encryption

**Version:** 1.0.1
**Status:** Stable
**Layer:** implementation
**Implements:** l1-security.md, l1-memory-model.md

## Overview

At-rest encryption for the memory store: each memory chunk is encrypted with AES-256-GCM, keyed from user credentials via the Argon2id key derivation function. The derived key is held only in process memory; the OS keychain stores an encrypted copy for session resume without re-entry of credentials. Key rotation re-encrypts all chunks transactionally.

## Related Specifications

- [l1-security.md](l1-security.md) - SEC-1 secret isolation, SEC-5 no leakage.
- [l1-memory-model.md](l1-memory-model.md) - Memory scopes and lifecycle this encryption layer wraps.
- [l2-memory-store.md](l2-memory-store.md) - The storage layer whose chunks are encrypted by this spec.
- [l2-security.md](l2-security.md) - OS keychain access pattern (same mechanism as §4.1 secret handling).
- [l2-filesystem-layout.md](l2-filesystem-layout.md) - State tier paths where encrypted memory lives.

## 1. Motivation

Memory entries contain summaries of past conversations, user preferences, and task history — sensitive content that must be protected if the device is lost or a process escapes its sandbox. Encryption at rest ensures that direct access to the database file yields no plaintext. The OS keychain provides a second factor: the content key is only accessible to the authenticated user session.

## 2. Constraints & Assumptions

- Encryption is per-chunk, not per-database. This allows partial decryption and deferred key unlock.
- The content key is never written to disk in plaintext; it lives in process memory for the lifetime of the session, and its only persisted copy is the OS keychain entry (§4.2), protected by the OS session credentials.
- Argon2id parameters are tuned for interactive use (≤ 500 ms KDF on 2024-era hardware).
- Key rotation requires the current session's key (already in memory), not a re-entry of credentials.

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| SEC-1 Secret isolation | The content key lives in process memory and, for session resume, in the OS keychain only (§4.2); it is never written to disk in plaintext, logged, or included in exports or backups. |
| SEC-5 No leakage | Plaintext exists only during the decrypt-read-use-discard cycle; it is never persisted after the read completes. |
| MEM-5 Scope-aware decay & prune | Encrypted chunks follow the same decay, prune and deletion lifecycle as plaintext chunks. |
| MEM-7 Ownership split | The encryption layer sits beneath the core read/write/recall contract and is transparent to the memory curator; no caller handles ciphertext. |

## 4. Detailed Design

### 4.1 Key derivation

The content key for a workspace is derived from the user's password and a workspace-specific salt:

```text
[REFERENCE]
key = Argon2id(
  password = user_password_bytes,
  salt     = hex(workspace_id)[0..32],  // 16-byte truncated workspace UUID as salt
  m        = 65536,                      // 64 MiB memory cost
  t        = 3,                          // 3 iterations
  p        = 4,                          // 4 parallelism threads
  key_len  = 32                          // 256-bit output
)
```

The salt is derived from the workspace identifier so the same password produces different keys for different workspaces. It is not a secret and does not need to be stored separately.

The derived key is **never stored on disk in plaintext** — its one persisted copy is the OS keychain entry of §4.2. After derivation, it is placed in a zeroizing buffer in process memory and zeroed when the session ends.

### 4.2 OS keychain integration

For session resume without re-entering the password, the content key is stored in the OS keychain under the entry `cronus:memory:<workspace_id>`:

- **On first unlock**: after Argon2id derivation, the 32-byte key is stored in the keychain (protected by OS session credentials).
- **On subsequent session starts**: the key is loaded directly from the keychain, skipping Argon2id. The password is not required again for the same OS session.
- **On keychain miss** (new device, key evicted, OS credential reset): the user is prompted for their password to re-derive.
- **Keychain backend**: platform-native — Windows Credential Manager, macOS Keychain, Linux Secret Service (libsecret). Falls back to in-process-only key (no persistence across restarts) if no keychain is available.

### 4.3 Encryption scheme

Each memory chunk is encrypted independently with AES-256-GCM:

```text
[REFERENCE]
EncryptedChunk {
  id:           String,       // same as the plaintext chunk's ID
  workspace_id: String,
  nonce:        [u8; 12],     // random, unique per chunk, never reused
  ciphertext:   Vec<u8>,      // AES-256-GCM encrypted content (includes GCM tag appended)
  aad:          Vec<u8>,      // additional authenticated data (not encrypted)
  created_at:   i64,          // Unix ms
}
```

**Nonce**: 12 bytes of cryptographically random data generated per chunk. Nonces are never reused; each new chunk (including re-encryption during rotation) gets a fresh nonce.

**AAD** (additional authenticated data): `b"cronus:memory:" + workspace_id.as_bytes()`. The AAD authenticates the binding between ciphertext and workspace — decrypting a chunk with the correct key but in the wrong workspace fails authentication.

**Plaintext format**: a JSON-serialized `MemoryChunk` (from `l2-memory-store.md`) compressed with zstd before encryption.

### 4.4 Encrypt / decrypt cycle

```text
[REFERENCE]
encrypt(chunk: MemoryChunk, key: [u8; 32]) -> EncryptedChunk:
  plaintext          = zstd_compress(json_serialize(chunk))
  nonce              = random_bytes(12)
  aad                = b"cronus:memory:" + chunk.workspace_id.as_bytes()
  (ciphertext, tag)  = aes_256_gcm_seal(key, nonce, plaintext, aad)
  // tag (16 bytes) is appended to ciphertext in the stored blob
  return EncryptedChunk { id: chunk.id, workspace_id, nonce, ciphertext: [ciphertext||tag], aad, created_at }

decrypt(enc: EncryptedChunk, key: [u8; 32]) -> MemoryChunk:
  plaintext = aes_256_gcm_open(key, enc.nonce, enc.ciphertext, enc.aad)
              // Err on tag mismatch: wrong key, tampered ciphertext, or wrong workspace
  chunk     = json_deserialize(zstd_decompress(plaintext))
  return chunk
```

Decryption failures surface as `DecryptionError { chunk_id, reason }` and are logged at WARN; they never produce partial plaintext.

### 4.5 Key rotation

Key rotation re-encrypts all chunks under a new key derived from a new password:

```text
[REFERENCE]
rotate_key(current_key: [u8; 32], new_password: &str, workspace_id: &str) -> RotationStats:
  new_key = Argon2id(new_password, salt=hex(workspace_id)[0..32], …)
  BEGIN TRANSACTION
    for each EncryptedChunk in workspace:
      plaintext = decrypt(chunk, current_key)   // Err aborts transaction
      new_enc   = encrypt(plaintext, new_key)
      upsert(new_enc)
  COMMIT
  keychain_store("cronus:memory:{workspace_id}", new_key)
  zero(current_key)
  zero(new_key)
  return RotationStats { chunks_rotated, elapsed_ms }
```

The transaction guarantees atomicity: if rotation fails mid-way (decrypt error, disk full), the database rolls back to the pre-rotation state (old key still valid).

### 4.6 Key lifecycle

| Event | Key action |
| --- | --- |
| Session start (keychain hit) | Load from keychain into zeroizing buffer |
| Session start (keychain miss) | Derive via Argon2id; store in keychain; hold in buffer |
| Session end / logout | Zero the in-process buffer; keychain entry persists |
| Workspace delete | Zero buffer; delete keychain entry; wipe memory DB and `codegraph.db` |
| Key rotation | Derive new key; re-encrypt (transactional); update keychain; zero both keys |
| OS lock screen | Zero buffer; keychain entry persists. The key is re-loaded from the keychain at its next use, so the always-on office keeps running unattended behind a locked screen (SCH-6, OFF-8); where the platform keychain is not readable while locked, a run that needs memory defers that step and reports it rather than failing silently. |

### 4.7 Command surface

| Action | CLI | TUI | Library (no code) |
| --- | --- | --- | --- |
| initial setup | `cronus memory encrypt --workspace <id>` | `/memory encrypt` | `memory.encrypt(workspace_id) -> void` |
| rotate key | `cronus memory rekey` | `/memory rekey` | `memory.rekey(new_password) -> RotationStats` |
| verify integrity | `cronus memory verify` | `/memory verify` | `memory.verify() -> VerifyReport` |
| encryption status | `cronus memory encryption-status` | `/memory encryption-status` | `memory.encryption_status() -> EncryptionStatus` |

## 5. Drawbacks & Alternatives

- **Password required on keychain miss**: on a new device or after OS credential reset, the user must re-enter their password. There is no recovery path without the password — this is by design (SEC-1).
- **Argon2id at 64 MiB / 3 iterations**: chosen for ≤ 500 ms on 2024-era hardware. Constrained devices may reduce `m = 32768`, `t = 2`; this requires a rotation for existing users to migrate to the lower parameters.
- **Compression before encryption**: exposes content-length statistics via ciphertext size — acceptable for a local store not subject to network traffic analysis.
- **Alternative — database-level encryption (SQLCipher)**: encrypts the whole database file rather than individual chunks. Simpler key management but coarser granularity: all memory is locked/unlocked together, and selective re-keying per-chunk is not possible. Per-chunk encryption supports future per-entry key diversity and partial unlock.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[SECURITY]` | `.design/main/specifications/l1-security.md` | SEC-1 and SEC-5 invariants |
| `[MEMORY]` | `.design/main/specifications/l1-memory-model.md` | Memory lifecycle |
| `[MEMSTORE]` | `.design/main/specifications/l2-memory-store.md` | Chunk storage this wraps |
| `[SEC2]` | `.design/main/specifications/l2-security.md` | Keychain integration pattern |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.1 | 2026-09-23 | Core Team | Consistency pass (2026-09-23): Resolved the "key never written to disk" vs "key stored in the OS keychain" contradiction (never on disk in plaintext; the keychain entry is its only persisted copy). Compliance row cited MEM-3 (multi-signal recall) for lifecycle — now MEM-5 and MEM-7. OS-lock row no longer waits for unlock, which would have stopped the unattended always-on office (SCH-6, OFF-8). |
| 1.0.0 | — | Core Team | Last version before this section was added; earlier revisions are recorded in version control. |
