# Command reference

[Documentation hub](README.md) · [Configuration guide](configuration.md) · [Project README](../README.md)

Examples use `bfsort` on your `PATH`. After a local build, use
`target/release/bfsort` instead. Put command options after the command name.

## Global presentation and configuration

`-c/--config FILE` uses exactly that file. Otherwise commands loading runtime
configuration check only `./bfs.toml`, then use built-in defaults. Explicit CLI
flags override TOML. All relative paths resolve from cwd. `--verbose` reports the
active file. Unknown fields and invalid values are errors.

`--color auto|always|never` defaults to auto (TTY only); `NO_COLOR` disables auto.
Explicit always/never override the environment; `--no-color` overrides all color
modes. `--json` disables ANSI/progress on both streams. `--quiet` suppresses normal
output, never errors; `--verbose` enables diagnostics. Compact live progress appears
only on interactive stderr, at most 8 redraws/second. Parallel transfers share a
single line; single-worker known-length transfers show byte percentage, unknown
lengths show bytes. Summaries include counts, failures, bytes, elapsed and output.

`collect --dry-run` never downloads payloads or writes acquisition state. A missing
inventory or `--refresh` requires an HTML metadata request. Otherwise inventory is
reused, including offline dry-run selection. Normal collect saves missing/refreshed
inventory and downloads only; it never ingests or unseals.

Inventories/receipts from previous releases are unsupported. To start explicitly
fresh without deleting old data: `bfsort collect URL -o ./collection-new`. There is
no migration. Download filename templates are exactly `{original_name}` (default),
`{name}-{source_id}`, `{source_id}`. `[download].directory` is a literal subfolder.
Unsafe basenames are sanitized and duplicate destinations fail before writes.
Discovery always shows original filenames, one per line in DOM order. JSON is
`{"files":["z.zip","a.zip"]}`. Normalized duplicate URLs keep the first link.
Remote page order may change. Internal inventory retains URLs/IDs; it has no
`title` or `discovered_at`. Explicit name ordering affects transfers only.

## Commands

| Command | What it does | Example |
| --- | --- | --- |
| `collect [URL] [-c FILE] [-o ROOT] [-j N]` | Discover when inventory is absent, then select and download. `--refresh` forces discovery. | `bfsort collect -c collection.toml` |
| `organize [INPUTS]... [-o PATH] [--extract]` | Same engine and options as ingest; ordinary files by default. | `bfsort organize ./incoming -o ./organized` |
| `config validate [-c FILE]` | Validate TOML and security/limit settings without processing inputs. | `bfsort config validate` |
| `discover [URL] [--config <FILE>]` | Scan one HTML page and save ordered ZIP links; no resource bodies downloaded. | `bfsort discover --config collection.toml` |
| `download [--config <FILE>]` | Apply policies and transfer ZIPs into sealed staging. | `bfsort download --config collection.toml` |
| `download [--config <FILE>] --dry-run` | Offline candidate selection preview; no requests or downloads. | `bfsort download --config collection.toml --dry-run` |
| `ingest [INPUTS]...` | Sort ordinary files, including ZIPs unchanged; use --extract for contents. | `bfsort ingest ./incoming --output ./organized --rules ./rules.toml` |
| `ingest --input-mode files [INPUTS]...` | Sort ordinary files and folders using the same policies. ZIP files are sorted as files, without extracting them. | `bfsort ingest ./documents --input-mode files --output ./organized --rules ./rules.toml` |
| `ingest --zip [INPUTS]...` | Sort inputs and publish only a compressed ZIP, without keeping a repository. Works with either input mode. | `bfsort ingest ./documents --input-mode files --zip --output ./sorted.zip --rules ./rules.toml` |
| `inspect [INPUTS]...` | Print ZIP entry metadata (use --json for machine output) without extracting payloads. Folders are searched for ZIP archives. | `bfsort inspect ./incoming --recursive false --json` |
| `stats [REPOSITORY]` | Print repository counts, duplicates, categories and storage usage (use --json for machine output) without reading payload contents. | `bfsort stats ./organized --json` |
| `reorganize [REPOSITORY] [--rules <FILE>]` | Build a new view from stored metadata using new rules, without unlocking payloads. Previous views are retained. | `bfsort reorganize ./organized --rules ./rules.toml` |
| `help [COMMAND]` | Show general help or help for a command; `--help` / `-h` also works. | `bfsort help ingest` |
| `--version` / `-V` | Print the installed version. | `bfsort --version` |

## Discovery and download options

| Command | Option | Required / accepted values | Effect |
| --- | --- | --- | --- |
| `discover` | `[URL]` | Required unless source.page is configured | Page URL; overrides `source.page` |
| `discover` | `--config <FILE>` | Optional runtime TOML | Overrides built-in discovery/transfer settings |
| `discover` | `--dry-run` | Parsed but rejected | Discovery requires requests; use download preview for offline selection |
| `download` | `--config <FILE>` | Optional; omitted settings use defaults | Reads the saved discovery inventory |
| `download` | `--dry-run` | Optional flag | Shows selected candidates without network or staging writes |
| Both | `--help`, `-h` | Optional | Shows command help |

Transfer settings such as speed, request spacing, ordering and concurrency are
TOML settings; `collect -j` overrides concurrency and `collect -o ROOT` selects ROOT/inventory.jsonl and ROOT/downloads. See the
[configuration guide](configuration.md) for field ownership and worker settings.

## Defaults without configuration

| Command | Default behavior |
| --- | --- |
| `bfsort discover URL` | Scans the supplied page and writes `./collection/inventory.jsonl`; no built-in website |
| `bfsort download` | Reads that inventory and downloads to `./collection/downloads`; 3 workers, no delay, no speed/request/count/size caps |
| `bfsort download --dry-run` | Offline selection from the default inventory |
| `bfsort ingest` | Uses completed download receipts when the default control directory exists; otherwise reads `./incoming`; writes sealed `./organized` |
| `bfsort inspect` | Inspects ZIPs in `./incoming` without extracting them |
| `bfsort stats` | Summarizes `./organized` |
| `bfsort reorganize` | Adds a new default-policy view to `./organized`; use `--rules` to change policy and `--dry-run` to preview |

Explicit paths and configured inputs win. Custom download paths require the same
configuration for ingest. Downloaded files still require explicit unlocking before
ingest; absent input directories/repositories/inventories produce errors. Unlimited
download defaults are bounded by filesystem capacity and integer representation;
finite network timeouts (300 seconds per transfer), origin checks, sealed permissions,
and extraction limits remain active. No automatic retries or partial-file resume.

## Common workflows

| Goal | Commands / next step |
| --- | --- |
| Download a web collection | `bfsort discover --config collection.toml`, then `bfsort download --config collection.toml` |
| Sort downloaded contents | Explicitly unlock all receipt-owned ZIPs to `0600`, then `bfsort ingest --config collection.toml` |
| Preview before writing | Download: `--dry-run` selects candidates offline. Ingest: `--dry-run` reads payloads and plans final destinations. |
| Sort local files | `bfsort ingest ./documents --input-mode files --rules examples/rules.files.example.toml --output organized` |
| Read ZIP metadata only | `bfsort inspect ./incoming` |
| Change an existing repository layout | `bfsort reorganize ./organized --rules rules.toml --dry-run`, then omit `--dry-run` |

## Ingest options

| Argument or option | Meaning | Default / accepted values |
| --- | --- | --- |
| `[INPUTS]...` | One or more file or folder paths. Explicit paths replace configured inputs. | Configured `inputs`, or completed download receipts when no explicit inputs exist. |
| `--config <FILE>` | Load runtime settings from TOML. Explicit CLI settings override file settings. | Built-in settings when omitted. |
| `--input-mode <MODE>` | Choose archive extraction or ordinary-file sorting. | `files` (default), `archives`; `--extract` selects archives. |
| `--output <PATH>` | Repository directory, or destination ZIP path when ZIP output is enabled. | `organized`; use an explicit ZIP filename with `--zip`. |
| `--rules <FILE>` | Load the central sorting policy: filename policy, ordered matchers, templates and duplicate handling. | Built-in rules: first occurrence per content hash, grouped by file type, named `{sha256}.sample`. Use `examples/rules.files.example.toml` to retain ordinary filenames. |
| `--zip` | Enable ZIP-only output. Refuses existing destinations and failed or interrupted runs. | Disabled. Can also be enabled with `zip = true` in config. |
| `--zip-dir <DIRECTORY>` | Select a sorted relative directory including descendants; repeat to select several. Enables ZIP output automatically. Selections are exact and case-sensitive. | All sorted entries when omitted. CLI selections replace configured `zip_dirs`. |
| `--zip-each-dir` | Create one ZIP per selected directory; without selections, one per top-level sorted directory. Enables ZIP output; `--output` becomes a new directory. | Disabled. Config: `zip_each_dir = true`. |
| `--output-password-file <FILE>` | Encrypt output contents with AES-256. The file must have `0600` permissions and contain exactly one nonempty password line. | No output encryption. Requires ZIP output. |
| `--staging-dir <DIR>` | Select an existing directory for private ZIP/preview working storage. Symlink paths are refused; a private child is created and cleaned up. Repository ingestion keeps its internal staging location. | ZIP: output parent. Preview: system temporary directory. CLI overrides `staging_dir` in config. |
| `--password-file <FILE>` | Read candidate passwords for encrypted input archives, one per nonempty line, from a `0600` file. Independent of the output password. | No candidates. |
| `--workers <N>` | Set parallel ingest workers. | Available CPU parallelism capped at 8; accepts `1`–`256`. |
| `--security-mode <MODE>` | Set repository payload permissions. Existing repository modes cannot be changed automatically. ZIP output always uses disposable private, standard-mode storage. | `sealed` (default, `0000` payloads), `standard` (`0600` payloads). |
| `--recursive <BOOL>` | Include nested folders or only immediate children. Pass the boolean explicitly. | `true` (default), `false`. |
| `--dry-run` | Hash/classify inputs in sealed temporary storage and print final destinations and duplicate counts; --json includes source mapping. Include existing repository metadata without publishing changes. | Disabled. Reads full contents; archive previews need input passwords. Files can change between preview and execution. Use `inspect` for metadata-only archive inspection. |
| `--help` / `-h` | Show ingest help. | — |

## Other command options

| Command | Argument or option | Meaning |
| --- | --- | --- |
| `inspect` | `[INPUTS]...` | Files or folders to search for ZIP archives; replaces configured inputs. |
| `inspect` | `--config <FILE>` | Load inputs, recursive discovery and archive limits from TOML. Does not switch inspection to ordinary-file mode. |
| `inspect` | `--recursive <BOOL>` | Override recursive discovery with explicit `true` or `false`; default is `true`. |
| `stats` | `<REPOSITORY>` | Existing repository; defaults to `./organized`. A ZIP output is not a repository. |
| `reorganize` | `<REPOSITORY>` | Existing repository; defaults to `./organized`. |
| `reorganize` | `--rules <FILE>` | Optional TOML policy; omitted uses built-in sample sorting rules. |
| `reorganize` | `--dry-run` | Print the proposed layout (--json for machine output) without publishing a new view. |
| All commands | `--help` / `-h` | Show help for that command. |

## Selecting sorted directories

```sh
# One ZIP containing only these sorted directories.
bfsort ingest ./documents --input-mode files --rules ./examples/rules.files.example.toml \
  --zip-dir documents/pdf --zip-dir text --output ./selected.zip

# Separate ZIPs for every top-level sorted directory.
bfsort ingest ./documents --input-mode files --rules ./examples/rules.files.example.toml \
  --zip-each-dir --output ./exports

# Separate ZIPs only for selected directories.
bfsort ingest ./documents --input-mode files --rules ./examples/rules.files.example.toml \
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
bfsort ingest ./documents --input-mode files --rules ./examples/rules.files.example.toml \
  --zip --output ./private.zip --output-password-file ./output-password.txt
```

Sources and password files remain untouched. Password files and their hard links
are excluded from discovered inputs. ZIP filenames remain visible even when
contents are encrypted; extraction requires an AES-compatible reader.

Policies are configured in [examples/rules.example.toml](../examples/rules.example.toml); runtime
settings and size/count limits are in [examples/config.example.toml](../examples/config.example.toml).
The default `organization.filename_policy = "samples"` requires `.sample`
filenames. For regular files, use [examples/rules.files.example.toml](../examples/rules.files.example.toml):
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

`bfsort discover URL` uses built-in defaults and writes `./collection/inventory.jsonl`.
Add `--config FILE` for custom limits or paths; an explicit URL overrides `source.page`.
`bfsort download` needs no config: it reads the default inventory and infers the
source origin from its first candidate. Use the same config for discovery and
download when enabling cross-origin links or custom paths.

`bfsort discover --config FILE` scans one HTML page, resolves direct `.zip` links,
deduplicates fragment-free URLs, and atomically saves a versioned inventory.
The inventory filename may end in `.jsonl` for compatibility with the example,
but its content is one versioned JSON object, not an append-only JSONL stream.
No payload is downloaded. Page failures preserve the previous inventory.

`bfsort download --config FILE --dry-run` reads the inventory without HTTP requests
or staging writes and prints selected candidates. `bfsort download --config FILE`
applies policies, downloads files and writes completion receipts. Reports contain
candidate source IDs and errors; local placement is also saved in receipts under
`staging_dir/control`. Payloads live under `staging_dir/payloads`.

`bfsort ingest --config FILE` without explicit inputs loads all completed receipts,
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
| `download.max_files` | No default cap; configure a positive count to limit attempts |
| `download.max_file_size` | No default cap; configure a readable size or integer bytes |
| `download.max_total_size` | No default cap; configured budget includes failed transfers |
| `download.max_staging_size` | No default cap; configured budget includes managed payloads and partials |
| `download.concurrency` | `3`, range 1–256 |
| `download.timeout_seconds` | `300` per transfer; network timeouts also bound individual requests |
| `download.max_speed` | Omitted = unlimited; e.g. `"2 MB/s"`, aggregate across workers |
| `download.min_request_interval_ms` | `0` = no spacing, shared across workers |
| `download.max_requests` | Omitted = unlimited; redirects count, cache/dry-run do not |
| `download.order_by` | `"source"` (DOM order) or `"name"` |
| `download.order` | `"ascending"` or `"descending"` |
| `download.filename` | `"{original_name}"`; also `"{name}-{source_id}"`, `"{source_id}"` |
| `download.directory` | Empty by default, or a safe literal relative folder under staging payloads |

Size settings accept integer bytes or strings with B/KB/MB/GB/TB (decimal),
or KiB/MiB/GiB/TiB (binary). Fractions, unitless strings, overflow and zero
resource limits are rejected. Speed accepts these units followed by `/s`, or
integer bytes per second; it limits application body reads, not exact
TCP traffic. Request caps reset between command invocations. No persistent scheduler.

Policies use `name_contains`, and `action`
`"include"` or `"exclude"`. Matching is case-insensitive. Exclusions win; if any
include policy exists, at least one must match. Download priority never changes the
saved inventory order. Distinct URLs with the same target basename fail preflight;
choose an ID filename template to distinguish them.

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

At an aggregate byte/staging cap, unknown-length responses fail closed if EOF
cannot be verified without another read. Leave budget headroom for such servers.
All completed receipted payloads must be explicitly unlocked before implicit
receipt-driven ingest; it does not select a subset of completion receipts.

Extraction `[limits]` fields `max_entry_bytes`, `max_archive_bytes`, and
`max_input_bytes` accept integer bytes or readable sizes such as `"256 MiB"`
and `"1 GiB"`. MB/GB use decimal units; MiB/GiB use binary units.
`max_entries` and `max_expansion_ratio` remain unitless integers.

Rule matchers `min_size` and `max_size` also accept readable sizes or integer
bytes (for example `max_size = "64 KiB"` or `max_size = 65536`). Numeric zero
remains valid for rule bounds. Reports and stored metadata retain numeric bytes.

## Detection reports and repository exports

```sh
bfsort report mismatches ./organized --format table
bfsort report mismatches ./organized --format json --all
bfsort report mismatches ./organized --format jsonl --status mismatch,ambiguous
bfsort report mismatches ./organized --format csv -o mismatches.csv --min-confidence high
bfsort analyze ./standard-repository
bfsort export ./standard-repository --rules examples/rules.formats.example.toml --zip-each-dir -o ./by-format
bfsort export ./standard-repository --rules examples/rules.extensions.example.toml --zip-each-dir -o ./by-extension
bfsort export ./organized -o ./preview.zip --dry-run
```

`report` reads stored metadata only; defaults show definitive mismatches.
`--all`, `--status` and `--min-confidence` control rows. JSON has `counts` and
`rows`; JSONL/CSV contain rows only. Output paths never overwrite existing files.
`analyze` explicitly refreshes classification into a sidecar, using stored objects.
`export` uses rules and persisted metadata without extraction, hashing or detection.
It preserves existing views; `--zip-dir`, `--output-password-file` and `--dry-run`
are supported. Analysis and actual payload exports require standard repositories;
sealed repositories permit reports and export previews. See the
[detection guide](detection.md) for bounds, schemas, statuses and compatibility.
