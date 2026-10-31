# Command reference

[Documentation hub](README.md) · [Project README](../README.md)

Examples use `bfs` on your `PATH`. After a local build, use
`target/release/bfs` instead. Put command options after the command name.

## Commands

| Command | What it does | Example |
| --- | --- | --- |
| `ingest [INPUTS]...` | Extract ZIP inputs, classify their contents, deduplicate and sort into a repository. Sources remain untouched. | `bfs ingest ./downloads --output ./repository --rules ./rules.toml` |
| `ingest --input-mode files [INPUTS]...` | Sort ordinary files and folders using the same policies. ZIP files are sorted as files, without extracting them. | `bfs ingest ./documents --input-mode files --output ./repository --rules ./rules.toml` |
| `ingest --zip [INPUTS]...` | Sort inputs and publish only a compressed ZIP, without keeping a repository. Works with either input mode. | `bfs ingest ./documents --input-mode files --zip --output ./sorted.zip --rules ./rules.toml` |
| `inspect [INPUTS]...` | Print ZIP entry metadata as JSON without extracting payloads. Folders are searched for ZIP archives. | `bfs inspect ./downloads --recursive false` |
| `stats <REPOSITORY>` | Print repository counts, duplicates, categories and storage usage as JSON without reading payload contents. | `bfs stats ./repository` |
| `reorganize <REPOSITORY> --rules <FILE>` | Build a new view from stored metadata using new rules, without unlocking payloads. Previous views are retained. | `bfs reorganize ./repository --rules ./rules.toml` |
| `help [COMMAND]` | Show general help or help for a command; `--help` / `-h` also works. | `bfs help ingest` |
| `--version` / `-V` | Print the installed version. | `bfs --version` |

## Ingest options

| Argument or option | Meaning | Default / accepted values |
| --- | --- | --- |
| `[INPUTS]...` | One or more file or folder paths. Explicit paths replace configured inputs. | Configured `inputs`; at least one input is needed. |
| `--config <FILE>` | Load runtime settings from TOML. Explicit CLI settings override file settings. | Built-in settings when omitted. |
| `--input-mode <MODE>` | Choose archive extraction or ordinary-file sorting. | `archives` (default), `files`. |
| `--output <PATH>` | Repository directory, or destination ZIP path when ZIP output is enabled. | `repository`; use an explicit ZIP filename with `--zip`. |
| `--rules <FILE>` | Load the central sorting policy: filename policy, ordered matchers, templates and duplicate handling. | Built-in rules: first occurrence per content hash, grouped by file type, named `{sha256}.sample`. Use `rules.files.example.toml` to retain ordinary filenames. |
| `--zip` | Enable ZIP-only output. Refuses existing destinations and failed or interrupted runs. | Disabled. Can also be enabled with `zip = true` in config. |
| `--zip-dir <DIRECTORY>` | Select a sorted relative directory including descendants; repeat to select several. Enables ZIP output automatically. Selections are exact and case-sensitive. | All sorted entries when omitted. CLI selections replace configured `zip_dirs`. |
| `--zip-each-dir` | Create one ZIP per selected directory; without selections, one per top-level sorted directory. Enables ZIP output; `--output` becomes a new directory. | Disabled. Config: `zip_each_dir = true`. |
| `--output-password-file <FILE>` | Encrypt output contents with AES-256. The file must have `0600` permissions and contain exactly one nonempty password line. | No output encryption. Requires ZIP output. |
| `--staging-dir <DIR>` | Select an existing directory for private ZIP/preview working storage. Symlink paths are refused; a private child is created and cleaned up. Repository ingestion keeps its internal staging location. | ZIP: output parent. Preview: system temporary directory. CLI overrides `staging_dir` in config. |
| `--password-file <FILE>` | Read candidate passwords for encrypted input archives, one per nonempty line, from a `0600` file. Independent of the output password. | No candidates. |
| `--workers <N>` | Set parallel ingest workers. | Available CPU parallelism capped at 8; accepts `1`–`256`. |
| `--security-mode <MODE>` | Set repository payload permissions. Existing repository modes cannot be changed automatically. ZIP output always uses disposable private, standard-mode storage. | `sealed` (default, `0000` payloads), `standard` (`0600` payloads). |
| `--recursive <BOOL>` | Include nested folders or only immediate children. Pass the boolean explicitly. | `true` (default), `false`. |
| `--dry-run` | Hash/classify inputs in sealed temporary storage and print final destinations, source mapping and duplicate counts as JSON. Include existing repository metadata without publishing changes. | Disabled. Reads full contents; archive previews need input passwords. Files can change between preview and execution. Use `inspect` for metadata-only archive inspection. |
| `--help` / `-h` | Show ingest help. | — |

## Other command options

| Command | Argument or option | Meaning |
| --- | --- | --- |
| `inspect` | `[INPUTS]...` | Files or folders to search for ZIP archives; replaces configured inputs. |
| `inspect` | `--config <FILE>` | Load inputs, recursive discovery and archive limits from TOML. Does not switch inspection to ordinary-file mode. |
| `inspect` | `--recursive <BOOL>` | Override recursive discovery with explicit `true` or `false`; default is `true`. |
| `stats` | `<REPOSITORY>` | Required path to an existing repository. A ZIP output is not a repository. |
| `reorganize` | `<REPOSITORY>` | Required path to an existing repository. |
| `reorganize` | `--rules <FILE>` | Required TOML sorting policy. |
| `reorganize` | `--dry-run` | Print the proposed layout as JSON without publishing a new view. |
| All commands | `--help` / `-h` | Show help for that command. |

## Selecting sorted directories

```sh
# One ZIP containing only these sorted directories.
bfs ingest ./documents --input-mode files --rules ./rules.files.example.toml \
  --zip-dir documents/pdf --zip-dir text --output ./selected.zip

# Separate ZIPs for every top-level sorted directory.
bfs ingest ./documents --input-mode files --rules ./rules.files.example.toml \
  --zip-each-dir --output ./exports

# Separate ZIPs only for selected directories.
bfs ingest ./documents --input-mode files --rules ./rules.files.example.toml \
  --zip-dir documents/pdf --zip-dir text --zip-each-dir --output ./exports
```

Selections refer to destinations produced by the sorting rules, after duplicate
handling. `--zip-dir documents` includes nested directories; a parent and child
selection include each entry only once in a combined ZIP. In separate mode,
each explicitly selected directory has its own archive, so overlapping selections
can repeat files across archives.

Separate mode writes `exports/documents/pdf.zip` and `exports/text.zip` for those
selections. Each archive preserves the complete sorted paths and includes only
its own entries in `layout.jsonl`. All archives use the same optional output
password. Unselected files are omitted from ZIP output; sources remain untouched.
Paths must be safe relative directories, without `..`, absolute paths, or glob
patterns. Missing directories and conflicting output paths are errors.

`--dry-run` includes the selected layout and an `archives` array showing output
paths and archive entries. Separate exports are built in private temporary
storage beside the destination and published together as a new output directory.
Existing files or output directories are refused. As with ZIP-only output, no
permanent repository is retained.

## Password-protected ZIP example

Create a local password file with `0600` permissions, then run:

```sh
bfs ingest ./documents --input-mode files --rules ./rules.files.example.toml \
  --zip --output ./private.zip --output-password-file ./output-password.txt
```

Sources and password files remain untouched. Password files and their hard links
are excluded from discovered inputs. ZIP filenames remain visible even when
contents are encrypted; extraction requires an AES-compatible reader.

Policies are configured in [rules.example.toml](../rules.example.toml); runtime
settings and size/count limits are in [config.example.toml](../config.example.toml).
The default `organization.filename_policy = "samples"` requires `.sample`
filenames. For regular files, use [rules.files.example.toml](../rules.files.example.toml):
`filename_policy = "regular"` with `default_filename = "{original_name}"`.
Names are sanitized and collision identifiers preserve extensions. Archive
inputs and archive-derived records refuse the regular policy, including legacy
records without input-mode provenance.

ZIP mode needs temporary disk space for uncompressed, deduplicated inputs plus
the ZIP. Normal completion and handled errors remove staging; an uncatchable
kill or crash may leave private plaintext under `.bfs-zip-*` in the selected
staging directory (by default, beside the output).
See the [ZIP usage notes](../README.md#sort-regular-folders-and-zip-the-result).

## Preview and ZIP reports

| Field | Meaning |
| --- | --- |
| Preview `plan` | Final paths, content hashes, occurrence IDs, source paths and original entry names after duplicate/collision handling. |
| Preview `summary` | Combined occurrences, unique files and duplicate occurrences, including existing repository metadata. |
| Preview `archives` | Output ZIP paths and their selected entries; present in ZIP modes. |
| Report `first_failure` | First failing source, optional entry index and cause; omitted on success. ZIP failures print this detail before cleaning temporary reports. |
| Report `zip.payload_bytes` | Total bytes of policy-selected payload entries, counting repeated entries if duplicate policy retains them. |
| Report `zip.archives` | Output path, payload bytes and final bytes for each ZIP. Aggregate ZIP byte/savings fields cover all exported archives, including repeated files in overlapping separate selections. |
| Report `zip.output_bytes` | Actual published ZIP size, including layout metadata and ZIP overhead. |
| Report `zip.saved_bytes` | Payload bytes minus final ZIP bytes. Negative means growth. |
| Report `zip.savings_percent` | Percentage saved relative to payload bytes; `null` when payload bytes are zero. |
| Report `zip.ingest_seconds` | Time spent ingesting and organizing before ZIP creation. |
| Report `elapsed_seconds` | Full ZIP run, including discovery, compression, publication and cleanup. |
| Report `summary` | Existing ingest repository statistics; these do not measure the final ZIP. |

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Command succeeded. |
| `1` | Runtime error, or ingest failed/interrupted without accepting any files. ZIP publication failures return this code. |
| `2` | Repository ingest accepted some files but also encountered failures or interruption; CLI argument errors also use this code. |

## Acquisition commands

`bfs discover --config FILE` scans one HTML page, resolves direct `.zip` links,
deduplicates fragment-free URLs, and atomically saves a versioned inventory.
The inventory filename may end in `.jsonl` for compatibility with the example,
but its content is one versioned JSON object, not an append-only JSONL stream.
No payload is downloaded. Page failures preserve the previous inventory.

`bfs download --config FILE --dry-run` reads the inventory without HTTP requests
or staging writes and prints selected candidates. `bfs download --config FILE`
applies policies, downloads files and writes completion receipts. Reports contain
candidate source IDs and errors; local placement is also saved in receipts under
`staging_dir/control`. Payloads live under `staging_dir/payloads`.

`bfs ingest --config FILE` without explicit inputs loads all completed receipts,
requires explicit unlocking to 0600, verifies hashes, then uses existing ingest.
It never ingests inventory/control files or partial transfers. Use `--dry-run` for
the existing full ingest preview. Unreceipted partials are ignored and count toward
staging usage. Missing or altered receipted payloads fail validation.

All paths are relative to the working directory. Existing root-level ingest
settings and CLI overrides remain supported; acquisition tables are optional.

| Setting | Default / meaning |
| --- | --- |
| `source.page` | Required HTTP(S) HTML page |
| `source.same_origin` | `true`; applies to links and redirect hops |
| `source.max_page_size` | `"5 MB"` |
| `download.inventory` | `"./collection/inventory.jsonl"` |
| `download.staging_dir` | `"./collection/downloads"` |
| `download.max_files` | `100` non-cached candidates attempted per invocation |
| `download.max_file_size` | `"1 GB"` |
| `download.max_total_size` | `"10 GB"` body bytes per invocation, including failed transfers |
| `download.max_staging_size` | `"20 GB"` managed payload bytes, including partials |
| `download.concurrency` | `3`, range 1–256 |
| `download.timeout_seconds` | `300` per transfer; network timeouts also bound individual requests |
| `download.max_speed` | Omitted = unlimited; e.g. `"2 MB/s"`, aggregate across workers |
| `download.min_request_interval_ms` | `0` = no spacing, shared across workers |
| `download.max_requests` | Omitted = unlimited; redirects count, cache/dry-run do not |
| `download.order_by` | `"title"`, also `"name"` or `"date"` (discovery timestamp) |
| `download.order` | `"ascending"` or `"descending"` |
| `download.directory` | `"{title}"`, `"{name}"`, or safe literal relative path |

Sizes accept positive integers with B/KB/MB/GB/TB (decimal), or KiB/MiB/GiB/TiB
(binary). Fractions, missing units, overflow and zero limits are rejected. Speed
uses the same units followed by `/s`; it limits application body reads, not exact
TCP traffic. Request caps reset between command invocations. No persistent scheduler.

Policies use exactly one of `title_contains` or `name_contains`, and `action`
`"include"` or `"exclude"`. Matching is case-insensitive. Exclusions win; if any
include policy exists, at least one must match. Title is normalized visible anchor
text with URL-basename fallback. Stable source IDs break ordering ties and prevent
staging-name collisions. Publication dates and surrounding headings are not inferred.

Redirects are capped at five and HTTPS downgrades are refused. HTTP 429 blocks
new requests to that origin for the invocation and reports numeric Retry-After;
there are no automatic retries. URLs with credentials or queries are unsupported.
No JavaScript, crawling, authentication, nested ZIP recursion, SFTP, range resume,
automatic cleanup, or non-ZIP extraction. Existing extraction limits still apply
independently and can be smaller than the download limits.

Downloaded payloads are sealed 0000 from creation through publication, directories
0700 and control files 0600. Owner/root can change permissions; this is not a noexec
mount. No automatic permission changes unlock payloads. Failed transfers do not
receive completion receipts; reruns validate sealed cache metadata and ingest checks
full hashes after explicit unlocking. Simultaneous download writers are locked out.

Exit codes: 0 success (including intentional policy filtering), 2 mixed success and
incomplete/failed work, 1 failure without success. Budget-blocked work is incomplete.
