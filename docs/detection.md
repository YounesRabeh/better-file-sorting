# Content detection and mismatch reports

bfsort records content evidence independently of the filename. A mismatch is a
filename/content inconsistency, not a malware verdict. Detection never executes,
renders, follows links, or recursively extracts content. Permissions are not a sandbox.

## Stored fields and compatibility

Each occurrence retains its original name, final original extension, archive entry,
source provenance, SHA-256, size and identity. New `detection` metadata contains
`detected_family`, `detected_format`, optional `mime_type`, `confidence`, `evidence`,
`alternatives`, and `detector_version` (`bfs-1`). New `mismatch` metadata contains
`status` and `reason`. Detection is performed before the staging handle is sealed
and published, using the prefix captured during the existing hash/copy pass.

`file_type` and `{file_type}` use the broad eight-category classifier and
meaning. They are intentionally independent of the stricter detailed detector:
for example an incomplete ELF prefix can retain legacy `file_type = "elf"` while
its detailed format is `unknown`. Use `detected_format` for precise content grouping.
Old records default to `not_evaluated` for detection and mismatch, never `match`.
Older bfsort binaries do not accept the new fields; use this version for new manifests.

Explicitly analyze an old **standard-mode** repository:

```sh
bfsort analyze ./repository
```

This reads each unique object once for classification (without rehashing), then
atomically replaces `reports/classification.jsonl`. Original journals and payloads
remain unchanged; metadata readers overlay only detection and mismatch fields.
Analysis is cancellable and an interrupted analysis leaves the previous sidecar
intact. Repeating analysis refreshes all records using the current detector.
Sealed repositories are refused; bfsort never changes payload permissions automatically.

## Supported identifiers and evidence

| Family | Format identifiers |
| --- | --- |
| executable | `pe`, `elf`, `macho`, `wasm` |
| document | `pdf`, `rtf`, `cfb`, `docx`, `xlsx`, `pptx`, `html`, `xml` |
| script | `javascript`, `powershell`, `python`, `shell`, `batch` |
| archive | `zip`, `rar`, `7z`, `tar`, `gzip`, `bzip2`, `xz` |
| package | `jar`, `apk` |
| image | `png`, `jpeg`, `gif`, `webp`, `bmp`, `tiff` |
| database | `sqlite` |
| text | `json`, `text` |
| other states | `unknown`, `ambiguous`, `not_evaluated` |

Confidence describes detection evidence, not safety:

- `high`: a strong signature with bounded header checks, a recognized shebang,
  or a supported ZIP directory structure.
- `medium`: a ZIP prefix without a validated bounded directory, a text structural
  preamble, complete JSON, or conflicting interpretations.
- `low`: printable UTF-8 without stronger evidence.
- `unknown`: no sufficient evidence, or a legacy unevaluated record.

PE requires DOS and PE headers with bounded offset and optional-header checks;
`MZ` alone is insufficient. ELF checks identification/header fields. Mach-O supports
thin headers; Java's `CAFEBABE` is not inferred to be a Mach-O executable. PDF
headers are searched within the first 1 KiB. TAR checks the header checksum.
Script detection relies on recognized interpreters in shebangs; ordinary prose
and language-looking filenames do not establish a language. Batch/HTML/XML/JSON
use conservative preambles/parsing and medium confidence.

ZIP subtypes use central-directory names: OOXML requires `[Content_Types].xml`
and `word/document.xml`, `xl/workbook.xml` or `ppt/presentation.xml`; JAR requires
`META-INF/MANIFEST.MF`; APK requires `AndroidManifest.xml` and either `classes.dex`
or `resources.arsc`. An APK's JAR manifest is compatible, not a conflict. Conflicting
subtype markers produce `ambiguous` with alternatives. Container markers do not
validate document semantics or authenticity.

## Bounds and limitations

The fast path retains the existing 8 KiB prefix per worker. Only ZIP candidates
use additional reads through the already-open handle: at most a 65,557-byte tail
plus a 1 MiB directory, 4,096 entries and 4,096 bytes per name. Names are retained
only within those limits. No entry is decompressed during subtype detection.
This adds bounded transient directory/name allocations per ZIP worker, not a
separate full-file read. Oversized, truncated, ZIP64 and multi-disk directories
remain generic ZIP. There is no mandatory new library, C runtime, external
executable or signature database; `infer` remains in the legacy classifier.

A signature is not full structural validation. Deeply embedded headers, binaries
with PE headers beyond the prefix, unsupported fat Mach-O variants, non-UTF-8 or
truncated text, scripts without strong markers, and many polyglots can remain
unknown, generic or undetected. `ambiguous` covers observed conflicting headers
and subtype markers; it does not promise general polyglot discovery.

## Extension compatibility

Normalization is case-insensitive, ignores leading dots, and uses the final
extension. `.tar.gz`, `.tar.bz2`, and `.tar.xz` are recognized as compound suffixes
for evaluation and statistics; the unmodified final extension remains in metadata.
Double extensions have no independent verdict: `invoice.exe.pdf` is checked as
PDF, and `document.pdf.exe` as EXE.

The registry includes EXE/DLL/SYS/SCR/OCX/CPL → PE; ELF/SO → ELF; DYLIB/MACHO →
Mach-O; DOC/XLS/PPT/MSI → CFB; DOCX/DOCM, XLSX/XLSM, PPTX/PPTM → the respective
OOXML family; JS/MJS/CJS → JavaScript; PS1/PSM1 → PowerShell; PY → Python;
SH/BASH/ZSH → shell; BAT/CMD → batch; HTML/HTM → HTML; XML/SVG → XML;
JSON → JSON; TXT/LOG/CSV/MD → text. Archive aliases include GZ/TGZ/TAR.GZ,
BZ2/TBZ2/TAR.BZ2 and XZ/TXZ/TAR.XZ. Image aliases include JPG/JPEG/JPE and
TIF/TIFF. SQLite accepts SQLITE/SQLITE3/DB. PDF/RTF/ZIP/RAR/7Z/TAR/JAR/APK/PNG/GIF/WEBP/BMP/WASM
use their named suffixes. Unlisted suffixes remain unknown.

| Status | Meaning |
| --- | --- |
| `match` | Registered extension agrees with identified format (EXE and DLL both match PE) |
| `compatible` | Generic BIN/DAT/TMP, or a ZIP extension on a recognized ZIP subtype |
| `mismatch` | High-confidence evidence contradicts a recognized extension |
| `missing_extension` | Filename has no extension |
| `unknown_extension` | Extension is not registered |
| `unknown_format` | Content evidence is insufficient |
| `ambiguous` | Conflicting interpretations, generic ZIP against a subtype extension, or evidence too weak to contradict the extension |
| `not_evaluated` | Legacy metadata not yet analyzed |

Generic text with `.js` is ambiguous; generic ZIP with `.docx` is ambiguous.
Neither produces a definitive mismatch. Extensionless files have their own status.

## Reports and statistics

```sh
bfsort report mismatches ./repository
bfsort report mismatches ./repository --format csv --output mismatches.csv
bfsort report mismatches ./repository --format json --output mismatches.json
bfsort report mismatches ./repository --format jsonl --all
bfsort report mismatches ./repository --status mismatch,ambiguous --min-confidence medium
bfsort stats ./repository --json
```

Reports default to definitive mismatches. `--all` includes every status; explicit
`--status` (repeatable or comma-separated) takes precedence. Confidence filtering
applies to rows. Status counts cover **all** occurrences before row filtering.
Table and JSON include those counts; CSV and JSONL contain selected rows only.
`--json` changes the default table to JSON; an explicit non-table format wins.
`--quiet` suppresses stdout, while `--output` still writes the requested artifact.
Output files are created atomically and never replace an existing destination.

JSON contains `counts` and `rows`; JSONL contains one row per line. Each row has
SHA-256, occurrence ID, source archive, original entry name/extension, detected
family/format, MIME, confidence, mismatch status/reason, detector version. CSV
quotes every data field and doubles embedded quotes; JSON uses serde serialization.
When importing untrusted CSV into spreadsheet software, import filename/source
columns as text so spreadsheet software does not interpret them as formulas.
Terminal output escapes control characters. Rows are sorted deterministically.

Statistics preserve all existing fields and add `by_detected_family`,
`by_detected_format`, `by_original_extension`, `by_mismatch_status`,
`extension_format`, and `mismatch_pairs`. Each bucket has `occurrences` and
`unique_hashes`. Cross tables are sparse extension/format rows, ordered by key;
`mismatch_pairs` contains definitive mismatch pairs. Unknown and ambiguous counts
are explicit buckets. One hash can occur under multiple extensions and statuses,
so summing unique counts across those buckets may exceed global unique samples.
Reports evaluate every occurrence before duplicate policies select exported files.

## Ingest once, export several views

For payload ZIP exports, create a standard repository explicitly:

```sh
bfsort ingest ./incoming --extract --security-mode standard --output ./repository
bfsort export ./repository --rules examples/rules.formats.example.toml --zip-each-dir --output ./by-format
bfsort export ./repository --rules examples/rules.extensions.example.toml --zip-each-dir --output ./by-extension
bfsort export ./repository --rules examples/rules.formats.example.toml --output ./all-formats.zip
bfsort export ./repository --rules examples/rules.formats.example.toml --zip-each-dir --output ./preview --dry-run
```

`export` loads existing objects and metadata. It never opens original daily ZIPs,
re-extracts, rehashes, reclassifies, or changes the repository's view pointer.
It reuses duplicate policies, safe path planning, ZIP directory selection and
atomic no-replace publication. `--zip-dir` and `--output-password-file` also work
as for ingestion. Parent output directories must already exist. Standard-mode
payload permissions are checked before opening each object. Export trusts stored
object identities and metadata; it does not perform an integrity audit.

Sealed repositories support metadata reports, `reorganize` and `export --dry-run`,
but payload export and analysis are refused. Existing objects and exports remain
unchanged when another view is exported. Archive-derived files keep `.sample`
names even if a rule attempts to select the regular filename policy.

Use `{detected_format}`, `{detected_family}`, or `{mismatch_status}` in templates,
and `detected_format` / `mismatch_status` in `[rules.match]`. Existing `{file_type}`
and `{original_extension}` templates retain their meanings. Legacy records sort
under `not_evaluated` until explicit analysis. Use `duplicates = "all"` when a
view must preserve every distinct source occurrence and filename association.

## Built-in presets and recovery

`by-format` is the default grouping; `by-extension` uses original suffixes and
`by-mismatch` uses filename/content agreement. Presets preserve every occurrence
and are mutually exclusive with custom `--rules`. See [exports](exports.md) for
the portable ZIP manifest and [recovery](recovery.md) for `--resume` verification.
Omitted extraction budgets are unlimited; the detector bounds above remain
mandatory technical limits. Use [bounded configuration](../examples/bounded.toml)
to opt into resource budgets.
