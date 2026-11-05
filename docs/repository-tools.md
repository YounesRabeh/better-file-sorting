# Repository tools

These commands operate on ingested repositories, rather than collection/download directories. Repository locks prevent concurrent writers. They preserve stored payload permissions and never unlock sealed objects. Use global `--json` for machine-readable results and `--no-progress` to suppress progress.

## Inspect stored metadata

```sh
bfsort verify REPOSITORY
bfsort verify REPOSITORY --full
bfsort query REPOSITORY --include-ext pdf,docx
bfsort query REPOSITORY --include-format pdf --count
bfsort diff LEFT RIGHT
```

`verify` checks stored structure, manifests, references, object sizes and permissions without repairs. `--full` additionally hashes referenced payloads and requires a standard repository. Sealed repositories support metadata verification only. Missing, corrupt and unsupported validation categories are reported; unsupported receipt schemas cannot establish integrity. A failed integrity check or an unavailable requested verification level returns failure.

`query` uses indexed, parameterized SQL over effective occurrence metadata, including classification results, without reading payloads. Default results describe occurrences; `--count` distinguishes selected occurrence count from distinct content hashes. Results have deterministic ordering.

`diff` compares LEFT with RIGHT: added and removed unique hashes, differing occurrence counts for shared hashes, and differing detection metadata. Detector versions accompany results. Distinct source events with identical bytes remain distinct occurrences; unrelated occurrence IDs are not treated as equivalent provenance.

## Selection

`query`, `sample`, `split` and `merge` accept `-c CONFIG` and the shared selection flags: `--include-ext`, `--exclude-ext`, `--include-format`, `--exclude-format`, `--sha256`, `--mismatch`, `--min-size` and `--max-size`. Extension and format lists accept repeated flags and comma-separated values. Extensions use original names; formats use effective content detection. Exclusions win. Nonempty extension and format include lists must both match.

The corresponding TOML `[filters]` lists are `include_extensions`, `exclude_extensions`, `include_formats` and `exclude_formats`. A supplied CLI list replaces its configured list; other lists still apply. There is no special CLI syntax to clear a configured list.

## Create independent repositories

```sh
bfsort sample REPOSITORY --count 100 --seed 7 -o SAMPLE --dry-run
bfsort sample REPOSITORY --count 100 --seed 7 -o SAMPLE
bfsort split REPOSITORY --by extension -o PARTITIONS
bfsort split REPOSITORY --by format -o FORMAT_PARTITIONS
bfsort merge LEFT RIGHT -o MERGED
```

Destinations must not exist and their parent directory must already exist. Source aliases and overlapping source/destination paths are rejected. Publication uses private staging and an atomic no-replace directory rename, currently supported on Linux. A failed copy leaves the destination unpublished; abrupt termination can leave a private staging directory in the destination parent. No resume option is provided.

Actual copying requires standard sources. A sealed source can participate in a metadata-only `--dry-run`, which publishes nothing and does not verify payload hashes. Actual copies stream and verify SHA-256 and size before publishing objects. Each destination stores each selected hash once, using copies rather than cross-repository hard links. Split partitions may independently contain copies of the same hash.

`sample` selects occurrences without replacement after filtering and rejects a count above the eligible population. Ranking is ascending SHA-256 over the bytes `bfsort-sample-v1\0`, the seed as an unsigned 64-bit big-endian integer, and the ASCII occurrence ID; ties use the occurrence ID. The same seed and effective selected population reproduce the same subset. The default seed is zero.

`split --by extension` uses the original extension, normalized to lowercase with leading dots removed. Group directory names are `ext-` followed by the UTF-8 bytes encoded as lowercase hexadecimal (for example, `txt` becomes `ext-747874`). Missing extensions use `unknown`. This encoding prevents source-controlled extensions from becoming paths. `--by format` uses the existing detected-format names, including explicit `unknown`, `ambiguous` and `not_evaluated` groups. Each group is a complete repository. The outer split directory is a container, not itself a repository.

`merge` preserves provenance and effective detection metadata. Identical occurrence identities and records deduplicate; conflicting records fail. Different occurrence IDs sharing a hash remain distinct occurrences. Historical run IDs remain provenance; no source run is rewritten as a newly completed operation.

Selected nested occurrences currently fail explicitly because ancestry closure has not been implemented. Choose a selection without nested occurrences until that contract is supported. Views, overlays, checkpoints, operation history and extraction receipts are not copied. Effective detection results are materialized in the destination SQLite catalog
and its JSONL compatibility manifest.

Dry-run reports selected occurrences, distinct objects, destination groups, estimated bytes to copy and ancestry additions (currently zero). Split byte estimates include an object's separate copies across groups. Sources remain unchanged by previews and copies.

## Filtering existing workflows

The same flags and `-c CONFIG` apply to `ingest`, `stats`, `report mismatches`,
`reorganize`, and `export`. With no filters, existing behavior is preserved.
An extension matches the original member/file name, never the safe `.sample`
view suffix. Optional leading dots and case are normalized. Lists within one
include dimension are ORed; nonempty dimensions and size/hash/mismatch criteria
are ANDed; exclusions win. Unknown content remains eligible unless restricted.
Use [examples/filters.toml](../examples/filters.toml) as a starting point.

In archive mode, the input ZIP is traversed and its members are selected after
extraction/detection and mandatory validation. Excluded members consume configured
budgets but are not published as objects/occurrences. In ordinary-file mode the
input ZIP itself is selected. Selection does not change source files or delete
previously ingested occurrences. Dry-run applies the same selection and tombstones.

Filtering combined with nested extraction is explicitly unsupported; it could
otherwise discard the parents needed to reach matching descendants. `analyze`
and `reclassify` do not expose shared selection flags. `discover`/`download` keep
existing resource-name policies and cannot evaluate formats inside archives.
Selection settings participate in ingestion checkpoint compatibility. A changed
selection reprocesses inputs rather than trusting an incomplete old selection.

`stats` applies selection to logical occurrence/category counts; stored bytes,
stored objects and orphan counts remain repository-wide. An orphan in this summary
means unreferenced by active occurrence metadata; retained views or checkpoints
can still pin it. Only prune's audited reachability decides reclamation.

## Operation history and retry

```sh
bfsort status ./repository --json
bfsort status ./collection --json
bfsort retry ./repository --operation OPERATION_ID
```

Actual persistent ingestion, collection/download, nested analysis, reclassification
and exports write versioned private records in the repository SQLite catalog;
collection roots retain JSON records in `operations/history/`. Small per-operation
OS lock files remain there for liveness. Dry-runs and direct disposable ZIP ingestion do
not create history. Old repositories without history return an empty list;
previous activity is not reconstructed as successful operations.

Records contain command arguments, application version, canonical working
directory, settings-file fingerprints, lifecycle timestamps, optional completed/
failed/skipped counts, checkpoint links and generic failure summaries. Counts are
recorded at completed processing boundaries, not on every progress redraw; a
crash may leave counts unavailable or behind the last durable checkpoint. Detailed
input failures remain in command reports. Configured passwords remain in protected
files; history never stores password contents or password digests.

Status does not take the repository lock and can inspect an active writer. A held
per-operation OS lock establishes `running`; an unfinished record without its
owner is reported as `interrupted` without changing the record. A handled failure
is `failed`; Ctrl+C records interruption where the cancellation path reports it.
Retry starts a linked operation in the recorded working directory and reuses the
original pipeline's verified `--resume` behavior. Configuration/rules fingerprints
and application version must still match; protected password files must be
supplied and valid again. Retry is whole-operation, not selected failed inputs.
Downloads restart incomplete bodies and ordinary partial archives restart from
the beginning. Existing output conflicts are preserved. Custom download staging
may require consulting the configured control path rather than the conventional
checkpoint link shown in history.

## Logical removal and orphan reclamation

```sh
bfsort remove ./repository --include-ext exe --dry-run
bfsort remove ./repository --include-ext exe --confirm
bfsort prune ./repository --dry-run
bfsort prune ./repository --confirm
```

Removal requires a nonempty selector and either dry-run or explicit confirmation.
It atomically records occurrence IDs in SQLite tombstones and emits a compatibility
`operations/removal.json` snapshot; original
manifests and provenance remain intact. Queries, views and exports use the active
metadata after tombstones. Reingesting the same occurrence identity does not
resurrect it. Use a new repository if a fresh ingestion is intended.

Removing an ancestor needed by a retained nested occurrence fails. Nested
extraction/analysis after logical removal is explicitly refused because receipt
replay could recreate removed descendants. Reclassification refreshes active
objects while preserving removed metadata without requiring reclaimed payloads.
Previously retained view generations remain usable and pin their data.

Prune validates known metadata schemas and computes conservative references from
active records, retained view layouts, classification metadata and recovery
receipts/checkpoints. Shared/hardlinked objects remain protected. Unknown,
malformed, unsafe or oversized metadata stops reclamation; cleanup never guesses
which references can be ignored. Running operations and conflicting repository
locks block cleanup. Protected stale data may remain intentionally; this command
does not expire view generations, operation history, receipts or snapshots.

Actual prune writes and syncs a deletion journal before unlinking only eligible
canonical objects, syncing each affected directory. After interruption, rerunning
recomputes references before continuing, so newly referenced data is protected.
Dry-run writes neither tombstones nor deletion journals. No command automatically
unseals a payload or deletes sources. This is conservative storage reclamation,
not secure erasure of file contents.

See [SQLite metadata](sqlite.md) for catalog authority, explicit migration and
JSONL import/export. Existing include/exclude flags keep the same meaning.
