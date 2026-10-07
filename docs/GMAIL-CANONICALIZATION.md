# Gmail canonical identity and consolidation

> Status: implementation and safety contract for the `gmail-canonical-mirror` branch.

## Why this exists

A Gmail message is commonly visible in more than one IMAP folder. Earlier archive behavior could represent those observations as multiple local Email rows or could adopt an existing row using weaker matching. Gmail exposes a stronger identifier: `X-GM-MSGID`.

This branch makes that identifier the authority for Gmail message identity.

## Identity collection

`sync_id_imap.gmail_msgid` stores `X-GM-MSGID` as text so the complete unsigned 64-bit range is preserved.

When Gmail advertises `X-GM-EXT-1`, normal IMAP metadata fetches request `X-GM-MSGID`. Existing tracked observations can be populated without downloading message bodies using:

```
vandelay import imap ... --gmail-identity-backfill ARCHIVE
```

Backfill is intentionally narrow:

- it requires an existing matching IMAP source;
- it requires `X-GM-EXT-1`;
- it cannot be combined with `--dry-run`;
- it updates only missing Gmail identities for already tracked folder/UID observations;
- it does not reconcile mailboxes, flags, membership, or message bodies;
- UIDVALIDITY mismatches and missing filtered folders are left unchanged rather than guessed.

## Canonicalization analysis

```
vandelay consolidate-gmail ARCHIVE
```

is read-only unless `--apply` is supplied. The analysis reports observation coverage, unique Gmail identities, duplicate identity groups, projected removed rows, blob conflicts, keyword conflicts, local rows spanning multiple Gmail identities, and before/after Email-row counts.

Automatic apply is blocked unless all tracked IMAP email observations have Gmail identity and there are no:

- differing RFC822 blobs within one Gmail identity;
- differing keyword state within one Gmail identity;
- local Email rows shared by multiple Gmail identities.

The last condition is a structural safety invariant. A local row spanning two Gmail identities indicates prior unsafe adoption and must be investigated rather than silently consolidated.

## Apply behavior

```
vandelay consolidate-gmail --apply ARCHIVE
```

must only be used after reviewing a clean analysis and taking/confirming a rollback checkpoint.

For each duplicate Gmail identity group, the current implementation:

1. retains the lowest local Email ID;
2. repoints IMAP observations from duplicate rows to the retained row;
3. rebuilds mailbox membership from the surviving IMAP observations;
4. preserves/remaps Takeout mappings where unambiguous and removes redundant mappings;
5. deletes JMAP export mappings for affected Email identities because old target mappings cannot be trusted after canonicalization;
6. deletes the duplicate local Email rows;
7. commits the operation transactionally.

The operation refuses to apply if the preflight analysis is unsafe.

## What must never be used as Gmail identity

Do not merge messages solely by:

- blob/content hash;
- RFC Message-ID;
- subject, sender, date, or other headers;
- apparent visual/content equality.

Historical investigation found real cases in which identical blobs belonged to different Gmail identities. That is why `X-GM-MSGID` is authoritative.

## Historical archive repair sequence

The intended controlled repair sequence is:

1. establish the Gmail identity schema and collection behavior;
2. backfill `X-GM-MSGID` for existing tracked IMAP observations;
3. verify complete identity coverage;
4. run read-only consolidation analysis;
5. inspect every blocker or anomaly;
6. confirm a rollback checkpoint;
7. apply canonicalization transactionally;
8. run post-apply integrity checks;
9. only then resume/validate normal synchronization and export behavior.

A small set of Takeout-specific untracked Spam artifacts was identified during investigation. That cleanup is separate from Gmail identity consolidation and must not be folded into canonicalization implicitly.
