# Presets and portable exports

[Documentation hub](README.md) · [Commands](commands.md) · [Recovery](recovery.md)

Ingest a mixed collection once, then export independent views without reopening
original archives or repeating classification:

```sh
bfsort ingest ./incoming --extract --security-mode standard -o ./repository --resume
bfsort export ./repository --preset by-format --zip-each-dir -o ./exports/by-format --resume
bfsort export ./repository --preset by-extension --zip-each-dir -o ./exports/by-extension --resume
bfsort export ./repository --preset by-mismatch --zip-each-dir -o ./exports/by-mismatch --resume
```

Each output is independent. Source objects, original inputs and existing view
pointers remain unchanged. Parent output directories are created privately when
needed. A sealed repository supports export preview, not payload export.

## Choose a preset

| Preset | Groups by | Example output ZIPs |
| --- | --- | --- |
| `by-format` (default) | Saved content detection | `pdf.zip`, `pe.zip`, `elf.zip`, `unknown.zip` |
| `by-extension` | Original final filename extension, case-insensitively | `pdf.zip`, `exe.zip`, `bin.zip` |
| `by-mismatch` | Saved extension/content agreement status | `match.zip`, `mismatch.zip`, `ambiguous.zip` |

Only nonempty groups produce ZIPs. Missing extensions use the sanitized `_` group.
By-extension grouping does not establish actual content: PE files named `.pdf`
remain in `pdf.zip`. By-format puts recognizable PE executables in `pe.zip`,
regardless of their original names. A mismatch is not a malware verdict.

Presets retain all source occurrences consistently, including repeated hashes.
If a hash appears more than once in a group, filenames receive deterministic
occurrence suffixes; the exported ZIP can therefore contain repeated content.
Use custom `--rules` when you want `first_only` or another duplicate policy.
Do not combine `--rules` and `--preset`.

## ZIP structure

```text
pdf.zip
├── pdf/
│   ├── <sha256>.sample
│   └── <sha256>-<occurrence-id>.sample
├── export.json
├── layout.jsonl
└── manifest.jsonl
```

`export.json` declares `kind = "bfsort-export"`, `schema_version = 1`, and tool
version. `layout.jsonl` maps exported paths to SHA-256 and occurrence identities.
`manifest.jsonl` contains rows shaped as:

```json
{"path":"pdf/<sha256>.sample","occurrence":{"sha256":"<sha256>","original_name":"document.pdf"}}
```

The example abbreviates the occurrence object. Actual records include the full
stored identity, size, original name/extension, archive entry, source provenance,
detection/MIME/confidence/evidence/version and mismatch decision. Metadata retains
original source paths, including local paths; consider that when sharing exports.
Passwords and encryption keys are never included. With output encryption,
metadata entries are encrypted with the payload entries.

For custom duplicate policies that omit repeated payloads, the manifest still
retains source occurrences associated with exported content and maps them to an
included payload path. This metadata contract supports future merge/split tools;
merge and split commands are not implemented.

Normal export trusts repository content identities; it does not rehash source
objects or perform an integrity audit. A consumer must verify payload hashes
before trusting a shared portable manifest. Resumable export hashes the completed
ZIP file to detect changes; recovery after publication without a saved receipt
also verifies entries and both metadata indexes. See [recovery](recovery.md).

## Storage requirements

An extracted repository retains unique object content. Hard-link views do not
add another payload copy. Each exported collection is an additional compressed
copy. Peak storage includes staging/output ZIPs as well as source archives and
repository objects. Compression ratios are data-dependent.
