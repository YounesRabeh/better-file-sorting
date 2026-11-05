# SQLite repository metadata

Repositories keep payloads as individual SHA-256-addressed files under `objects/`. SQLite stores metadata, relationships and operation history; it does not store payload bytes. The catalog is `metadata.sqlite3` inside the repository. Its presence makes it authoritative: editing, replacing or deleting an exported occurrence JSONL file does not change catalog records.

## Existing repositories

Legacy repositories without a catalog remain readable. Queries and dry-run previews load their existing JSONL metadata into temporary memory without creating a database. `metadata migrate` or the first supported metadata mutation migrates legacy effective records, including classification results and removal state, into a private catalog. Existing JSONL snapshots remain available for compatibility and interchange. They cannot silently override an established catalog.

Migration and supported metadata writes run under the existing exclusive repository lock. An unsupported schema version or invalid catalog fails closed; the application does not fall back to stale JSONL when a catalog exists. Keep a consistent backup before moving a repository between application versions. Automatic downgrade is not supported.

## JSONL interchange

```sh
bfsort metadata migrate REPOSITORY
bfsort metadata export REPOSITORY -o records.jsonl
bfsort metadata import REPOSITORY records.jsonl
```

Export writes active effective occurrence records, including current detection metadata. It publishes atomically and refuses to overwrite an existing output file. Export does not include removed occurrences, payloads, historical views, recovery checkpoints or operation history.

Import validates bounded records, identities and existing destination object paths, sizes and security modes. Objects must already be present in the destination repository; import does not copy or unlock payloads. A malformed record or conflicting occurrence identity rejects the import without publishing an accepted prefix. Equal records may be reused. Use ordinary repository-derived commands when both metadata and payloads need to be copied.

The existing include/exclude extension and format flags, hash, mismatch and size selectors retain their semantics. Selection uses bound SQL parameters. Original filename extensions and detected formats remain independent: an ELF object named `program.pdf` matches extension `pdf` and detected format `elf`.

## Privacy, durability and recovery

The repository and database are private. Metadata remains owner-readable/writable, including catalog journal or sidecar files. Sealed payload permission bits stay unchanged. A metadata query or catalog migration does not authorize opening sealed payload bytes.

Catalog writes enable foreign keys and full synchronization, with rollback journals.
Schema versions are explicit; numeric metadata must fit SQLite signed 64-bit
integers and oversized values are rejected rather than truncated.

Object publication precedes the metadata transaction: an interruption between those steps may leave an unreferenced object, rather than a record pointing at an unpublished object. A successful database commit is authoritative even if writing a compatibility JSONL snapshot subsequently fails; that failure is reported. Existing extraction receipts, export checkpoints and filesystem publication checks continue to govern payload recovery.

Back up or copy the entire repository while its writer is stopped and its repository lock can be acquired. Do not copy only the main SQLite file during a write; a live transaction can involve journal or sidecar state. Preserve the original payload tree, catalog and recovery metadata together. Use supported integrity verification before resuming work on a restored copy.

Do not delete `metadata.sqlite3` to rebuild a catalog from JSONL. Snapshots can be stale and omit committed classifications, removals or operation history; deleting the authority can resurrect old records or lose updates. Preserve the damaged repository and restore a consistent backup, or use a separately validated recovery procedure.

Repository operation records live in the catalog; `operations/history/` retains
small OS lock files for liveness. Collection/download roots keep their existing
JSON history because they are not ingested repositories. Legacy operation JSON
is imported during migration and cannot override database state afterward.
Retained view references enter SQLite before directory publication; a failed
publication may conservatively pin data until an explicit recovery procedure.
