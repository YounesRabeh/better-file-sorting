# Configuration guide

[Documentation hub](README.md) · [Commands and CLI options](commands.md)

## Choose the right file

| Example | Purpose | Used by | Start with |
| --- | --- | --- | --- |
| [examples/config.example.toml](../examples/config.example.toml) | Local inputs, output, ingest workers, security and extraction limits | `ingest --config`, `inspect --config` | Copy to `config.toml` for local files or ZIPs |
| [examples/collection.example.toml](../examples/collection.example.toml) | Web page, download policies, staging and transfer limits, plus eventual ingest output | `discover --config`, `download --config`, `ingest --config` | Copy to `collection.toml` and replace the example page URL |
| [examples/rules.example.toml](../examples/rules.example.toml) | Sorting extracted ZIP entries using safe `.sample` names | `ingest --rules`, `reorganize --rules`, or runtime `rules` field | Copy to `rules.toml` for archive inputs |
| [examples/rules.files.example.toml](../examples/rules.files.example.toml) | Sorting ordinary files while preserving filenames | Same rules options, with file-mode ingest | Copy to `rules.toml` for `--input-mode files` |

Runtime settings and sorting rules are separate schemas. Pass runtime files with
`--config`, and rules files with `--rules` or the runtime `rules` field. Unknown
settings are rejected. Relative paths resolve from the working directory, not the
configuration file's directory. Examples are editable presets, not a list of defaults.

## How settings are grouped

| Location | Controls | Does not control |
| --- | --- | --- |
| Runtime root | Input paths/mode, output, ingest workers, ZIP export, passwords, rule-file path | Download concurrency or per-site pacing |
| `[source]` | One HTML page, same-origin restrictions, maximum page size | Recursive crawling or file-transfer servers |
| `[download]` | Payload staging, ordering, concurrency, transfer budgets and pacing | Extracted-content destinations or extraction limits |
| `[[download.policies]]` | Which downloadable ZIPs are included/excluded by filename | Which entries inside a ZIP are selected by sorting rules |
| `[security]` | Repository permission mode and required safeguards | Automatic unlocking of downloaded payloads |
| `[limits]` | Compressed input, expanded entry/archive, count and expansion-ratio limits | Network speed or download budgets |
| Rules `[organization]` | Filename policy, duplicate handling, view type and fallback templates | Acquisition of remote files |
| Rules `[[rules]]` | Ordered matchers and destination overrides; first matching rule wins | HTTP requests or retry behavior |

## Workers and sequential stages

| Stage | Setting | Built-in default | Parallel work |
| --- | --- | --- | --- |
| Discovery | None | One page | Single-page parsing and discovery |
| Download | `[download].concurrency` | `3`, accepts `1`–`256` | Multiple resources; speed and budgets are shared |
| Ingest | Root `workers` / `--workers` | Available CPU parallelism capped at `8`; accepts `1`–`256` | Multiple ordinary files or ZIP archives; entries within one ZIP remain sequential |
| ZIP export | None | Sequential | Output ZIP writing is currently sequential |

Run stages in order: discover, download, explicitly unlock completed payloads,
then ingest. Workers are independent settings, not a pool shared between commands.
Network body reads currently share a budget lock, which can limit throughput.

## Local input and output settings

| TOML key | CLI override | Meaning / accepted values |
| --- | --- | --- |
| `inputs` | Positional input paths | Explicit CLI paths replace configured paths; if neither is set, ingest can use download receipts |
| `input_mode` | `--input-mode` | `files` (default) keeps ZIPs as ordinary files; `archives` or CLI `--extract` extracts contents |
| `output` | `--output` | Repository directory, new ZIP filename, or new separate-export directory |
| `rules` | `--rules` | Sorting policy path; does not load runtime configuration |
| `recursive` | `--recursive true/false` | Whether local discovery includes nested folders; default `true` |
| `workers` | `--workers` | Parallel input processing; see worker table above |
| `zip` | `--zip` | Produce ZIP output instead of retaining a repository |
| `zip_dirs` | Repeated `--zip-dir` | Select sorted directories and descendants; enables ZIP output |
| `zip_each_dir` | `--zip-each-dir` | Separate ZIPs for selections, or each top-level directory |
| `password_file` | `--password-file` | Input ZIP password candidates in a private `0600` file |
| `output_password_file` | `--output-password-file` | Exactly one output password in a private `0600` file; ZIP output required |
| `staging_dir` | `--staging-dir` | Existing scratch location for ZIP/preview temporary storage |

Root `staging_dir` is temporary ingest scratch. `[download].staging_dir` is the
persistent download area. They serve different purposes and need not be the same.
CLI values override runtime TOML, then built-in defaults supply omitted settings.
Without `-c`, only `./bfs.toml` is loaded if present; no parent directories are searched.
An explicit missing config is an error. `--verbose` prints the active path.
Both `organize` and `ingest` default to files.

`[download].filename` supports exactly `{original_name}` (default), `{name}-{source_id}`,
and `{source_id}`. Extensions appear once. `[download].directory` is a literal relative
folder beneath `staging_dir/payloads`, empty by default. Filename collisions fail
before payload writes; choose an ID template to disambiguate. `order_by = "source"`
is the default, with optional `"name"` affecting execution only. Policies use
`name_contains` and include/exclude actions. Old acquisition inventories/receipts
are rejected; explicitly start a fresh root with `bfsort collect URL -o collection-new`.

## Sizes, rates and extraction limits

| Value | Meaning |
| --- | --- |
| `"2 MB"` / `"2 MiB"` | Decimal megabytes / binary mebibytes |
| `2000000` | Integer bytes; supported as a size fallback |
| `"2 MB/s"` / `"2 MiB/s"` | Aggregate download bytes per second using decimal / binary units |
| `2000000` for `download.max_speed` | Integer bytes per second |
| Omitted `download.max_speed` | No application speed cap; request and concurrency limits still apply |

Supported size units: B, KB, MB, GB, TB, KiB, MiB, GiB, TiB. Strings require a
positive integer and a unit; fractions and overflow are rejected. Internal metadata
and reports remain numeric bytes. Counts, ratios, seconds and milliseconds are
unitless numbers in their explicitly named fields.

| `[limits]` key | Built-in default | What it bounds |
| --- | --- | --- |
| `max_entry_bytes` | `"256 MiB"` | One expanded ZIP entry or ordinary file |
| `max_archive_bytes` | `"1 GiB"` | Aggregate expanded bytes per ZIP; also bounds ordinary file size |
| `max_input_bytes` | `"1 GiB"` | Compressed ZIP input or ordinary file size |
| `max_entries` | `10000` | Entries per ZIP, or discovered file count in files mode |
| `max_expansion_ratio` | `1000` | Maximum entry expansion relative to compressed size |

All extraction limits must be positive; entry limit cannot exceed archive limit.
Download size acceptance does not guarantee acceptance by extraction limits.
See the [acquisition settings table](commands.md#acquisition-commands) for download
and page defaults, ordering, speed, request caps and retention behavior.

## Sorting-rule settings

| Section / key | Accepted values / purpose |
| --- | --- |
| Root `version` | `1` |
| `organization.filename_policy` | `samples` (default) or `regular` for ordinary-file workflows only |
| `organization.duplicates` | `first_only` (default), `all`, `only_duplicates`, `separate` |
| `organization.view_mode` | `hardlink` (default) or `manifest`; links retain stored payload permissions |
| `organization.default_directory` | Fallback directory template, default `{file_type}` |
| `organization.default_filename` | Fallback filename template, default `{sha256}.sample` |
| Rule `name` | Human-readable label |
| `rules.match.file_type` | Detected content type, such as `pdf`, `pe`, `elf` |
| `rules.match.detected_format` | Precise typed identifier such as `docx`, `png`, `python` |
| `rules.match.mismatch_status` | Typed status such as `mismatch`, `compatible`, `ambiguous` |
| `rules.match.original_extension` | Case-insensitive original extension, without a dot |
| `rules.match.encrypted` | Whether the input ZIP entry was encrypted |
| `rules.match.duplicate` | Whether the payload content has duplicate occurrences |
| `rules.match.min_size`, `max_size` | Inclusive byte bounds; readable sizes or integers; numeric zero is allowed |
| `rules.output.directory`, `filename` | Destination templates overriding organization defaults |

Matchers in one rule must all match. Template fields include `file_type`,
`original_extension`, `original_name`, `original_stem`, `sha256`, `size`,
`encrypted`, `duplicate`, `source_archive`, `ingest_date`, `occurrence_id`,
`detected_format`, `detected_family`, and `mismatch_status`.
See [detection](detection.md) for identifiers, confidence, legacy defaults and
metadata schemas. `examples/rules.formats.example.toml` and `examples/rules.extensions.example.toml`
provide independent export layouts.
Archive-derived records require `.sample` filenames. Paths are validated and
collisions resolved by the existing planner.

## No-config workflow

All commands support built-in operational defaults. Discovery still needs a URL;
other commands need existing input data, an inventory or a repository. See the
[default commands table](commands.md#defaults-without-configuration).
Download presets in `examples/collection.example.toml` intentionally impose limits; built-in
transfer defaults impose none. Pass a config to restore application byte/count caps.
Timeouts, origin restrictions, sealing and extraction limits remain in force.
Reorganize can omit `--rules` to use built-in policies rather than loading a file.

## Working directory layout

```text
working-directory/
├── incoming/                       # Optional manually supplied files or ZIPs
├── collection/
│   ├── inventory.jsonl             # Discovery inventory (versioned JSON)
│   └── downloads/
│       ├── control/                # Download lock and completion receipts
│       └── payloads/               # Sealed downloaded ZIPs in literal configured folders
└── organized/                      # Default managed bfsort repository
    ├── security.json
    ├── .lock
    ├── .staging/                   # Temporary ingest/view work
    ├── objects/                    # Content-addressed, deduplicated payloads
    ├── manifests/                  # Per-run occurrence metadata
    ├── reports/                    # Ingest summaries and failure journals
    └── views/                      # Layout generations, indexes and CURRENT pointer
```

`discover` writes the inventory; `download` adds payloads and receipts; `ingest`
creates the repository after explicitly unlocking downloaded inputs. `stats` reads
metadata and prints JSON without creating a new report file. `incoming` is the
fallback for local inputs, not an intermediate download directory.

The defaults were renamed from `downloads` and `repository`. Existing directories
are never renamed or moved. Use explicit input/output paths, a runtime config,
or `bfsort stats ./repository` to continue working with an older layout. Explicit
settings take precedence; `collection/downloads` remains unchanged.
