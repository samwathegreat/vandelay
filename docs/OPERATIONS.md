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

The current safety baseline before live consolidation work is commit:

`735872b395236d150b69d22ae0b19fd95284a66a`

The verified release binary built from that revision had SHA-256:

`7e6a1bc749f2a73f8f66a55387485cd5fcbdd40145ff2385653607ac35b05f9c`

This is historical provenance, not a promise that future builds will retain the same hash.

CI for the branch runs formatting, `cargo check --all-targets`, and `cargo test --all-targets`.

## Runtime history

Earlier development used a two-stage container pattern:

- an Alpine/Rust builder image;
- a small Alpine runtime image containing `/usr/local/bin/vandelay` and running as an unprivileged `vandelay` user.

The current verified binary was built using a Debian Bookworm Rust image and is dynamically linked against glibc. It must not simply be copied into the historical Alpine/musl runtime image. Runtime packaging should match the binary ABI or the binary should be rebuilt for the intended runtime.

Vandelay remains an on-demand tool; Stalwart and Bulwark are the persistent application services.

## Current rollback point

Immediately before the Gmail identity backfill work, the deployment created:

`vault/mail-archive@pre-x-gm-msgid-backfill-20261007`

Retain the relevant pre-mutation checkpoint until canonicalization and post-operation verification are complete. Snapshot names are deployment history; operators of other installations should create their own checkpoint.

## Backfill

Use the Gmail identity backfill mode only against the already-associated Gmail IMAP source. It is a write operation even though it does not download bodies or perform general reconciliation.

After backfill, verify that Gmail identity coverage reports zero missing observations before considering consolidation.

## Consolidation

First run:

```
vandelay consolidate-gmail /path/to/archive.sqlite
```

Do not use `--apply` until the report has been reviewed.

A safe report must have zero missing identities, zero blob-conflict groups, zero keyword-conflict groups, and zero local rows spanning multiple Gmail identities.

Only after those conditions and rollback protection are confirmed should:

```
vandelay consolidate-gmail --apply /path/to/archive.sqlite
```

be considered.

## Credentials and public output

Prefer environment variables or interactive secret input over command-line credentials. Never commit command transcripts containing real addresses, credentials, tokens, account identifiers, Gmail labels, message subjects, or message content.

When documenting diagnostics from a real archive, sanitize them before committing. Use invented labels such as `Receipts`, `Travel`, or `Projects` and addresses such as `user@example.com`.

## Manager status

A manager/orchestration container is planned for repeatable operation and health integration. Until that implementation is present and reviewed in source, these CLI procedures remain the operational reference.
