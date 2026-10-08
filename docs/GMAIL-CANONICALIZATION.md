# Gmail canonical identity and consolidation

> Status: implementation and safety contract for the `gmail-canonical-mirror` branch.

## Why this exists

A Gmail message is commonly visible in more than one IMAP folder. Earlier archive behavior could represent those observations as multiple local Email rows or could adopt an existing row using weaker matching. Gmail exposes a stronger identifier: `X-GM-MSGID`.

This branch makes that identifier the authority for Gmail message identity.

## Identity collection

`sync_id_imap.gmail_msgid` stores `X-GM-MSGID` as text so the complete unsigned 64-bit range is preserved.

IMAP ingestion on this branch requires `X-GM-EXT-1`. Normal IMAP metadata/body fetches request `X-GM-MSGID`, and a fetched message that omits the required Gmail identity is rejected rather than matched by blob equality. Existing tracked observations can be populated without downloading message bodies using:

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

is read-only unless `--apply` is supplied. Read-only analysis opens the SQLite archive with `mode=ro&immutable=1`; this permits analysis of Vandelay's WAL-mode archive through a strict read-only filesystem/container mount without creating SQLite sidecar files. Immutable mode assumes the database is stable/offline and must not be used against an archive that may be changing concurrently.

The analysis reports observation coverage, unique Gmail identities, duplicate identity groups, projected removed rows, blob conflicts, keyword conflicts, local rows spanning multiple Gmail identities, and before/after Email-row counts.

Automatic apply is blocked unless all tracked IMAP email observations have Gmail identity and there are no:

- differing RFC822 blobs within one Gmail identity;
- differing keyword state within one Gmail identity;
- local Email rows shared by multiple Gmail identities.

The last condition is a structural safety invariant. A local row spanning two Gmail identities indicates prior unsafe adoption and must be investigated rather than silently consolidated.

## Verified historical-archive findings

A sanitized read-only preflight of the migration archive found complete Gmail-identity coverage and no keyword conflicts or local Email row spanning multiple Gmail identities. It nevertheless found 28 Gmail-identity groups containing differing RFC822 blobs, so the archive remains unsafe to apply automatically.

Further read-only analysis found five content blobs referenced by separate local Email rows whose IMAP observations span two distinct `X-GM-MSGID` values. Each of those five blobs has one row with original Takeout provenance plus later IMAP-created rows. This is a distinct structural condition from one local Email row spanning multiple Gmail identities and explains why the existing `multi_identity_local_rows` check alone did not detect it.

Content inspection of the conflict set also found a historical case where two genuinely different messages reused the same RFC `Message-ID`. This is direct evidence that RFC `Message-ID` is not safe as Gmail logical identity and supports the branch rule that `X-GM-MSGID` is authoritative.

A cross-identity blob is not by itself proof of corruption: Gmail can in principle contain two logical messages with byte-identical RFC822 content. It is therefore a reconciliation/safety condition that must be understood before apply, not a reason to merge identities.

The current migration archive must remain blocked from apply while these findings are being reconciled. A future analysis revision should report cross-identity blob groups explicitly and treat unresolved groups as an apply blocker.

## Takeout provenance and forensic recovery

The Takeout importer records `sync_id_takeout.source_obj_id` for email objects as the hexadecimal BLAKE3 hash of the parsed MBOX message contents. It is not an RFC `Message-ID`, Gmail message ID, or SHA-256 identifier.

The Takeout MBOX parser retains message contents, including headers such as `X-GM-THRID`, in the stored RFC822 blob. The importer currently uses `X-Gmail-Labels` for mailbox/keyword classification but does not persist `X-GM-THRID` in a dedicated identity column. The MBOX envelope sender is parsed and retained by the parser, while the envelope date is used as the preferred internal/received date.

The original Takeout source remains useful forensic evidence because its message-content hash provides exact Takeout provenance and its stored raw headers can be compared with authoritative Gmail IMAP metadata. `X-GM-THRID` is thread identity, not message identity, and must never substitute for `X-GM-MSGID`.

Historical repair must not repeat weak RFC `Message-ID` matching. Exact blob/content equality may be used as provenance or correlation evidence, but never as the authority that declares two Gmail observations to be the same logical message.

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

Historical investigation found real cases in which identical blobs belonged to different Gmail identities. That is why `X-GM-MSGID` is authoritative. The content-addressed blob ID remains part of storage/deduplication, but it is not permitted as an IMAP message-identity key.

## Historical archive repair sequence

The intended controlled repair sequence is:

1. establish the Gmail identity schema and collection behavior;
2. backfill `X-GM-MSGID` for existing tracked IMAP observations;
3. verify complete identity coverage;
4. run read-only consolidation analysis;
5. inspect every blocker or anomaly, including cross-identity content;
6. reconcile historical identity associations using authoritative Gmail identity plus preserved provenance;
7. rerun analysis and require a clean safety report;
8. confirm a rollback checkpoint;
9. apply canonicalization transactionally;
10. run post-apply integrity checks;
11. only then resume/validate normal synchronization and export behavior.

A small set of Takeout-specific untracked Spam artifacts was identified during investigation. That cleanup is separate from Gmail identity consolidation and must not be folded into canonicalization implicitly.

## Post-consolidation controlled Gmail test checkpoint (October 2026)

The earlier preflight and unsafe-to-apply findings above describe a **historical pre-repair archive**, not necessarily the present deployed archive. Subsequent controlled production import testing used canonical Gmail identity mappings and reported complete populated `gmail_msgid` coverage; do not extrapolate the old projected duplicate-row counts to the current database.

A disposable two-message test created two canonical Email rows, each observed in five Gmail IMAP folders: Inbox, a custom test label, All Mail, Important and Sent Mail. In Stage 2 the custom label was removed from only one message. The next sync reported `new=0 vanished=1` for that label, `email.deleted=0`, and read-only SQLite inspection confirmed the affected canonical Email retained four IMAP observations while the untouched test Email retained five. This verifies label-removal reconciliation, not permanent-deletion preservation.

A concurrent-arrival hypothesis explains cases where Inbox reports zero new observations but All Mail reports new observations: mailbox enumeration is sequential, not an atomic snapshot. A Stage 2 run also reported a new Spam observation. Per-record logging is needed before attributing any individual observation to a specific arrival.

**Still unverified at this checkpoint:** moving a test message to Trash, subsequent permanent deletion, classification of Spam-origin messages, and behavior after removing the test label entirely. The proposed archive-owned preservation mailboxes and `--no-archive` behavior are future design, not implemented.

## Stage 3 verified: Gmail Trash move changes canonical local ID (2026-10-08)

A controlled test moved the second disposable Gmail message into Trash without permanently deleting it. The completed sync reported one vanished observation each in Inbox, the custom test label, All Mail, Important and Sent Mail, and one new observation in `[Gmail]/Trash`. The run summary reported `email: created=2 deleted=1` (the other new message was unrelated).

Read-only post-run inspection using **both original local Email IDs and both authoritative Gmail message IDs** established that the second test message retained its `X-GM-MSGID` but its local canonical Email ID changed from the original test row to a new row, with only the Trash mailbox membership. The first test message retained its original canonical ID and four observations. This establishes deletion/re-creation across a Trash transition; it does **not** establish byte-level payload equivalence or the exact internal processing sequence.

**Required future fix:** preserve canonical Gmail identity across moves between source mailboxes, including Trash, rather than deleting and recreating the Email when its old observations vanish before the new folder is processed. Review source reconciliation ordering and mapping/provenance impacts. Do not attempt manual production DB repair. Permanent deletion remains untested at this checkpoint.
