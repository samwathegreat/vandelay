# Gmail archive operations

> Status: operational guardrails. Commands shown here are templates and intentionally contain no real account data.

## Safety rules

The live archive is not a development fixture. Do not run mutating experiments against it.

Before a planned archive mutation:

1. identify the exact Vandelay source revision and binary/image being used;
2. confirm CI/tests for that revision;
3. run the applicable read-only analysis;
4. confirm a current filesystem/ZFS rollback checkpoint;
5. preserve the analysis output;
6. apply only the reviewed operation;
7. perform post-operation integrity checks before proceeding.

Do not infer success from a process exit alone when database identity is being changed.

## Build provenance

The current verified source/image baseline used for read-only consolidation analysis is source commit:

`5f0810f78f684a8f934c53e8d80f940f19c710cf`

The verified container image was tagged `vandelay-gmail:5f0810f`, with image ID:

`sha256:4fdf7a6ed825eea1988124475544a87794e64667bfe318c032bfa004555da224`

The binary inside that image had SHA-256:

`d2f8ba1edf4dcc453dd930d246866f518dfbd4dcce3e423730f6d4262a8ac884`

Earlier verified builds, including the original `735872b...` baseline and the intermediate `098c6b4` read-only build, are superseded for this operation. The latter used ordinary SQLite read-only mode, which is insufficient for a WAL-mode Vandelay archive mounted strictly read-only because SQLite may require sidecar access.

CI for the branch runs formatting, `cargo check --all-targets`, and `cargo test --all-targets`.

## Runtime history

Earlier development used a two-stage container pattern:

- an Alpine/Rust builder image;
- a small Alpine runtime image containing `/usr/local/bin/vandelay` and running as an unprivileged `vandelay` user.

The current verified binaries are built using a Debian Bookworm Rust image and dynamically linked against glibc. They must not simply be copied into the historical Alpine/musl runtime image. Runtime packaging should match the binary ABI or the binary should be rebuilt for the intended runtime.

Vandelay remains an on-demand tool; Stalwart and Bulwark are the persistent application services.

## Current rollback point

Immediately before the Gmail identity backfill work, the deployment created:

`vault/mail-archive@pre-x-gm-msgid-backfill-20261007`

Retain the relevant pre-mutation checkpoint until canonicalization and post-operation verification are complete. Snapshot names are deployment history; operators of other installations should create their own checkpoint.

## Backfill

Use the Gmail identity backfill mode only against the already-associated Gmail IMAP source. It is a write operation even though it does not download bodies or perform general reconciliation.

After backfill, verify that Gmail identity coverage reports zero missing observations before considering consolidation.

## Read-only consolidation analysis

First run:

```
vandelay consolidate-gmail /path/to/archive.sqlite
```

Do not use `--apply` until the report has been reviewed.

Read-only consolidation opens SQLite using `mode=ro&immutable=1`. This is intentional: Vandelay archives use WAL mode, and a strict read-only mount does not permit SQLite to create/access ordinary WAL sidecars as needed by a conventional read-only connection. Immutable analysis assumes the archive is stable and not being modified concurrently.

The verified migration preflight reported, in sanitized aggregate form:

- 591,848 tracked IMAP observations, all populated with Gmail identity;
- 270,744 unique `X-GM-MSGID` values;
- 270,456 duplicate-identity groups;
- 321,104 projected duplicate Email-row removals;
- 28 blob-conflict groups;
- zero keyword-conflict groups;
- zero local Email rows spanning multiple Gmail identities;
- a projected Email-row count reduction from 591,884 to 270,780;
- `safe_to_apply=false`.

Subsequent read-only investigation found five content blobs represented by separate local Email rows whose IMAP observations span two Gmail identities. Each of those five blobs includes an original Takeout-provenance row and later IMAP-created rows. This condition is not currently summarized by the `multi_identity_local_rows` metric and remains under reconciliation.

A safe report must have zero missing identities, zero blob-conflict groups, zero keyword-conflict groups, zero local rows spanning multiple Gmail identities, and no unresolved cross-identity content/provenance anomalies.

Only after those conditions and rollback protection are confirmed should:

```
vandelay consolidate-gmail --apply /path/to/archive.sqlite
```

be considered.

## Preserved forensic sources

During historical repair, retain the original Google Takeout source until canonicalization and post-operation verification are complete. The Takeout importer identifies an imported email object by the hexadecimal BLAKE3 hash of the parsed MBOX message contents and stores that value in `sync_id_takeout.source_obj_id`.

Takeout raw message data may contain `X-GM-THRID`, but thread identity is not Gmail message identity. It may be used as supporting forensic evidence only; `X-GM-MSGID` remains authoritative.

Do not repair historical mappings by RFC `Message-ID` alone. Investigation found genuinely different messages that reused the same RFC `Message-ID`.

## Credentials and public output

Prefer environment variables or interactive secret input over command-line credentials. Never commit command transcripts containing real addresses, credentials, tokens, account identifiers, Gmail labels, message subjects, or message content.

When documenting diagnostics from a real archive, sanitize them before committing. Use invented labels such as `Receipts`, `Travel`, or `Projects` and addresses such as `user@example.com`.

Aggregate operational counts and non-identifying structural findings may be documented when they do not expose archive content.

## Manager status

A manager/orchestration container is planned for repeatable operation and health integration. Until that implementation is present and reviewed in source, these CLI procedures remain the operational reference.
