# Resume and interruption recovery

[Documentation hub](README.md) · [Commands](commands.md)

`--resume` reuses verified completed work. It does not mean every operation can
continue from its last byte. Checkpoints are small private metadata files, not
additional copies of payloads.

## Ingestion

```sh
bfsort ingest ./incoming --extract -o ./repository --security-mode standard --resume
```

Successful inputs get versioned checkpoints under `operations/ingest/` after
objects and occurrence records are durable. A retry hashes input archive/file
bytes, checks processing settings and detector version, reconciles occurrence
metadata, and verifies stored objects. Standard objects are hash-verified. Sealed
objects remain unopened and rely on the existing permission boundary, size and
recorded identities; permissions do not protect against a malicious owner/root.

Changed input content or processing settings triggers reprocessing. Grouping
changes only rebuild the view. Partial or failed archives restart from the
beginning; already stored content is deduplicated. Passwords affect retries;
a successfully completed unchanged encrypted archive can be reused after its
input and stored results verify, without storing a password or password digest.
The supplied password file still needs to satisfy permission/format checks.

Verification reads the entire input. Newly processed resumable input is hashed
before and after extraction to detect mutation. Resume avoids extraction and
per-entry classification; it does not promise zero disk I/O.

Ingestion dry-run creates no durable checkpoints. Direct ZIP ingestion uses a
disposable repository and rejects `--resume`; use persistent ingestion then export.

## Downloads

Both normal downloads and `--resume` reuse validated completed receipts beneath
`collection/downloads/control/`. Receipts retain source identity, path, size and
content hash. Sealed cached files are checked for path, permissions and size;
explicitly accessible input is hash-verified before receipt-driven ingestion.
Files cannot be opened through sealed permissions just to validate a cache.

Failed or incomplete bodies restart, not continue via HTTP Range. Unreceipted
existing destinations are retained and reported as conflicts. No source files or
receipts are silently deleted to make a retry succeed.

## ZIP exports

```sh
bfsort export ./repository --preset by-format --zip-each-dir -o ./exports/by-format --resume
```

Each ZIP publishes atomically and gets a size/hash receipt in
`repository/operations/export/`. The checkpoint identifies the repository snapshot,
selected layout, output destination and encryption mode. Completed ZIPs are
hash-verified before reuse. Missing outputs can be regenerated; changed outputs
are rejected without replacement. Changed repository/layout settings require a
fresh destination. For encrypted outputs, the supplied password must also open
the exported metadata; no password or password digest is stored in checkpoints.

A crash between ZIP publication and saving its receipt is recovered by validating
payload hashes, the versioned export header, layout and full manifest before
recording completion. Unsupported or inconsistent checkpoints fail explicitly.

A resumable output directory can contain completed ZIPs while the collection is
incomplete. Rerun the same command to continue. Without `--resume`, multi-ZIP
output retains all-or-nothing directory publication and refuses existing targets.

## Cancellation and resource exhaustion

Ctrl+C stops new processing and preserves durable completed work. Handled disk-full,
quota and I/O failures retain committed objects and completed output ZIPs; private
temporary files are cleaned through normal error handling. Forced termination or
power loss can leave staging files; repository ingestion recovers abandoned run
staging under its exclusive lock. There is no claim of arbitrary OOM recovery or
power-loss fault-injection coverage.

Network waits use bounded-buffer workers and cancellation polling independently
of optional deadlines. An OS-stalled request/body worker can outlive a cancelled
operation until networking returns. It owns no output file and cannot keep the
CLI process alive. Cancellation is not a guarantee that the OS socket closes
immediately. No user deadline is imposed when `timeout_seconds` is omitted.

Entry-level extraction continuation, HTTP Range continuation, a dedicated resume
management command, and merge/split operations are not implemented.
