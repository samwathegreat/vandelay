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

The upstream Vandelay codebase supports many protocols. This project currently prioritizes the Gmail archive use case. IMAP ingestion on this branch intentionally requires a server advertising `X-GM-EXT-1`. Non-Gmail IMAP servers are rejected rather than falling back to weaker message identity. Generic IMAP compatibility may be restored or generalized later, but it must not weaken Gmail identity correctness.

## Public-documentation privacy

This repository is public. Documentation and fixtures must not contain real archive-derived email addresses, correspondent names, credentials, tokens, account identifiers, real Gmail label names, message subjects/content, or other personal mail data. Examples must use invented values. Aggregate, non-identifying operational statistics are acceptable.

## October 2026 live synchronization findings (verified / pending)

A controlled two-message Gmail test confirmed that removing a custom Gmail label removes only the corresponding IMAP observation and mailbox membership; the canonical Email remains associated with Inbox, All Mail, Important and Sent Mail. Another test message retained all five observations. This is **verified Stage 2 behavior**, not a claim about permanent deletion.

Gmail mailboxes are scanned sequentially. During a run, a message may arrive after Inbox was scanned but before All Mail; consequently per-folder `new` counts are not a single atomic Gmail snapshot. In one observed run, All Mail and Spam each gained an observation while Inbox gained none. Arrival timing is a plausible explanation, not proven per-message causality.

**Stage 3 pending:** moving the disposable second test message to Gmail Trash; verify actual IMAP observations before concluding whether Gmail retains Inbox/custom-label associations in Trash. The Gmail web UI alone does not establish IMAP folder membership.

## Planned preservation architecture — not implemented

The desired archive behavior is to retain a canonical message after it disappears completely from Gmail, by assigning it to an archive-owned mailbox such as `Deleted from Source`. The default should preserve; a future `--no-archive` option would opt into source-mirroring deletion. A separate `Deleted from Spam` mailbox should classify messages whose last known source state included Gmail Spam. Exact classification of a message formerly in Spam but subsequently moved through Trash requires an explicit historical-membership policy.

Reconcile the **entire selected source mailbox set** before deciding a canonical message has disappeared. Never classify a vanished individual folder/UID observation as whole-message deletion. Preserve Gmail identity and last-known associations independently of archive-owned preservation mailboxes; folder scan order must not determine classification. Verify handling of empty/skipped Trash folders and excluded folders before implementing permanent-deletion decisions. No preservation code is claimed deployed.
