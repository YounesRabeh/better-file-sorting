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
| `by-format-or-extension` | Medium/high-confidence detected content, otherwise original extension | `detected/pdf.zip`, `undetected/extension-foo.zip`, `undetected/no-extension.zip`, `ambiguous.zip` |
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

## Automatic grouping with optional explicit rules

```sh
bfsort export ./repository --preset by-format-or-extension --zip-each-dir -o ./exports/automatic --resume
```

No format/extension list is required. Medium/high-confidence identified formats
use `detected/<format>/`. Unknown, unevaluated and low-confidence content uses
`undetected/extension-<extension>/`, or `undetected/no-extension/` when no usable
extension exists. Conflicting detection uses `ambiguous/`. For example, generic
printable text has low confidence and falls back to its filename extension.
Fallback names are lowercase and sanitized; they do not establish content format.
Every occurrence is retained, including duplicate hashes. Only nonempty groups
produce ZIPs. New detector identifiers and arbitrary extensions become groups
automatically. The existing `by-format` default remains available unchanged.

With this grouping, `--zip-each-dir` creates a ZIP for each complete destination
directory (`detected/pdf.zip`, rather than a combined `detected.zip`). Custom rules
using `{format_or_extension}` get the same behavior, including their explicit
override groups. Other rules/presets keep their top-level ZIP grouping.
Explicit `--zip-dir` selections still include the selected directory's descendants.

For automatic fallback **and** explicit selections in one run, use
[rules.automatic.example.toml](../examples/rules.automatic.example.toml):

```toml
[organization]
duplicates = "all"
default_directory = "{format_or_extension}"

[[rules]]
name = "selected-pdfs"
[rules.match]
detected_format = "pdf"
[rules.output]
directory = "selected-pdfs"
filename = "{sha256}.sample"
```

Run with `--rules examples/rules.automatic.example.toml` instead of `--preset`.
The first matching rule controls its output; everything else gets the automatic
fallback. `{format_or_extension}` is a directory-only placeholder. Detection,
confidence and original extension remain separately recorded in the portable manifest.

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

`export.json` declares `kind = "bfsort-export"`, `schema_version = 2`, and tool
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

Schema 2 supports nested provenance in occurrence records. Each nested record
retains its parent occurrence/hash, root identity, depth and entry-index chain.
A parent may be outside the selected ZIP; hashes and IDs still preserve the link.
Older export schemas are rejected during resume verification.
