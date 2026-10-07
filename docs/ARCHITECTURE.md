# Gmail archive architecture

> Status: project-specific design notes for the `gmail-canonical-mirror` branch. This branch is currently optimized for Gmail archival correctness. Non-Gmail compatibility is not a current requirement.

## Purpose

The deployment is a searchable, self-hosted archive of a long-lived Gmail account. It combines:

- **Google Takeout** for the initial bulk historical seed.
- **Vandelay** as the archive/import/reconciliation engine.
- **Gmail IMAP** for ongoing synchronization and authoritative Gmail message identity.
- **Stalwart** as the mail/JMAP store used by the deployed archive.
- **Bulwark** as the webmail/search-facing UI.

Vandelay's SQLite archive is the reconciliation boundary between source ingestion and later export. Import and export are deliberately decoupled.

## Storage model

Vandelay stores object metadata in SQLite and message payloads in content-addressed blob storage. IMAP observations are tracked separately from canonical local Email rows.

A Gmail message can appear through multiple Gmail IMAP folders. Those folder/UID observations must be able to point to one canonical local Email.

The authoritative Gmail message identity is `X-GM-MSGID`, scoped to the IMAP source. RFC Message-ID and identical RFC822 bytes are not authoritative Gmail identity: two distinct Gmail messages may have identical content.

## Current identity invariant

For Gmail-backed observations:

1. One `(source_id, X-GM-MSGID)` identifies one logical Gmail message.
2. Many folder/UID observations may point to that one local Email.
3. One local Email must not span multiple distinct Gmail identities.
4. Two different Gmail identities must never be collapsed merely because their blobs, Message-ID values, or other content happen to match.
5. Canonicalization must not proceed while any tracked IMAP email observation is missing Gmail identity.

## Deployment model

Stalwart and Bulwark are persistent services. Vandelay has historically been built in a builder container and executed from a small runtime container as an on-demand utility; it is not currently a permanent service in the Stalwart/Bulwark Compose stack.

A future manager/orchestration layer is planned. That manager is not yet the authoritative implementation of archive operations and should not be confused with the current CLI workflow.

## Gmail-first scope

The upstream Vandelay codebase supports many protocols. This project currently prioritizes the Gmail archive use case. Generic IMAP/non-Gmail compatibility may be restored or generalized later, but it must not weaken Gmail identity correctness.

## Public-documentation privacy

This repository is public. Documentation and fixtures must not contain real archive-derived email addresses, correspondent names, credentials, tokens, account identifiers, real Gmail label names, message subjects/content, or other personal mail data. Examples must use invented values. Aggregate, non-identifying operational statistics are acceptable.
