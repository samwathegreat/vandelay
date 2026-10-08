# Project decisions

This file records decisions that are easy to lose when only preserved in chat history. Entries distinguish current decisions from historical/superseded behavior.

## Current decisions

### Gmail identity is authoritative

**Decision:** Use Gmail `X-GM-MSGID`, scoped by IMAP source, as the canonical identity for Gmail messages.

**Reason:** Different Gmail messages can contain identical RFC822 bytes. Content hash and Message-ID matching are therefore insufficient to decide Gmail identity.

### A canonical Email may have many IMAP observations

**Decision:** Permit multiple `(folder, UIDVALIDITY, UID)` observations to reference one local Email.

**Reason:** Gmail exposes one logical message through multiple folders/labels.

### A local Email may not span Gmail identities

**Decision:** Treat a local Email associated with more than one distinct Gmail identity as a blocker.

**Reason:** Silently accepting that state can destroy two real messages by collapsing them into one.

### Backfill before consolidation

**Decision:** Populate Gmail identity for existing tracked observations before attempting canonicalization.

**Reason:** Consolidation cannot safely infer identity for observations with missing authoritative IDs.

### Consolidation is explicit and gated

**Decision:** Canonicalization is a separate read-only-analysis / explicit-`--apply` operation, not an invisible side effect of routine sync.

**Reason:** It mutates established archive identity and must be reviewable and recoverable.

### Non-Gmail compatibility is not a current constraint

**Decision:** IMAP ingestion on this branch requires `X-GM-EXT-1` and `X-GM-MSGID`. Non-Gmail IMAP servers are rejected. Blob equality is not an allowed IMAP identity fallback.

**Reason:** The present project is a Gmail archival system. General non-Gmail support can be reconsidered later rather than weakening Gmail semantics now. Blob IDs remain valid content-addressed storage references, but not logical Gmail message identity.

### Public documentation is sanitized

**Decision:** Never publish real archive-derived addresses, names, credentials, account identifiers, Gmail label names, subjects, message content, or similar personal data. Use invented examples. Aggregate statistics are acceptable.

### Repository documentation is durable project memory

**Decision:** Keep these documents updated as implementation and operational knowledge changes. When history is ambiguous, clarify rather than guessing. If later evidence challenges an earlier design decision, discuss the change before replacing the documented decision.

## Historical / superseded behavior

Earlier IMAP adoption/matching behavior used weaker identities such as Message-ID and blob matching in circumstances where Gmail identity was not available. Those mechanisms helped bootstrap the archive but are not authoritative for Gmail canonicalization.

Earlier development images were based on a Clee-derived Vandelay revision and an Alpine builder/runtime pattern. They are historical tooling, not the current source baseline.

The upstream README describes Vandelay as a broad multi-protocol migration utility. That remains true of the inherited codebase, but it is not the scope constraint for this Gmail-focused branch.

## Planned, not yet authoritative

- Manager/orchestration container for repeatable jobs and health integration.
- Finalized production runtime packaging for the current Gmail-focused binary.
- Live Gmail identity consolidation after read-only verification.
- Separate cleanup of known Takeout-specific untracked artifacts.
- Possible future restoration/generalization of non-Gmail IMAP compatibility.

Do not document planned items as deployed behavior until they are implemented and verified.

## October 2026 accepted preservation and observability requirements (not yet implemented)

- **Preservation by default:** when a Gmail message truly disappears from the reconciled source, retain its canonical content in an archive-owned `Deleted from Source` mailbox. Future `--no-archive` is an explicit opt-out to true mirror deletion; exact CLI semantics and migration need implementation review.
- **Spam separation:** if the message's last known Gmail state included Spam, classify source disappearance into `Deleted from Spam` rather than ordinary deleted mail. Whether *historical* Spam membership survives a move through Trash remains a design question, not an implemented rule.
- **No false deletion on label changes:** removing a label or moving a message between source folders must only reconcile observations/memberships while the canonical Gmail identity is still present.
- **Whole-source reconciliation:** do not treat a per-folder `vanished` event as permanent deletion. Avoid scan-order-dependent classification, especially with Gmail's All Mail and Trash semantics.
- **Auditable manager:** future orchestration should log run IDs, per-message identity and membership transitions, classification decisions, per-folder timings/IMAP and SQLite phase metrics, errors and resource usage, with configurable rotation and retention. Logs must redact secrets and minimize personal-message data.
- **Documentation statuses:** distinguish observed/verified behavior, accepted future design, and unresolved tests. Do not describe the preservation feature or manager as shipped.

**Controlled test status:** Stage 2 verified a custom-label removal with canonical retention and no canonical deletions. Stage 3 (move to Trash) and Stage 4 (permanent deletion) were pending when this checkpoint was written. Do not predeclare their outcomes.
