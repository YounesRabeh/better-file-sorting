# Configuration guide

[Documentation hub](README.md) · [Commands and CLI options](commands.md)

## Choose the right file

| Example | Purpose | Used by | Start with |
| --- | --- | --- | --- |
| [config.example.toml](../config.example.toml) | Local inputs, output, ingest workers, security and extraction limits | `ingest --config`, `inspect --config` | Copy to `config.toml` for local files or ZIPs |
| [collection.example.toml](../collection.example.toml) | Web page, download policies, staging and transfer limits, plus eventual ingest output | `discover --config`, `download --config`, `ingest --config` | Copy to `collection.toml` and replace the example page URL |
| [rules.example.toml](../rules.example.toml) | Sorting extracted ZIP entries using safe `.sample` names | `ingest --rules`, `reorganize --rules`, or runtime `rules` field | Copy to `rules.toml` for archive inputs |
| [rules.files.example.toml](../rules.files.example.toml) | Sorting ordinary files while preserving filenames | Same rules options, with file-mode ingest | Copy to `rules.toml` for `--input-mode files` |

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
| `[[download.policies]]` | Which downloadable ZIPs are included/excluded by title or name | Which entries inside a ZIP are selected by sorting rules |
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
| `input_mode` | `--input-mode` | `archives` (default) extracts ZIPs; `files` keeps ZIPs as ordinary files |
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
| `rules.match.original_extension` | Case-insensitive original extension, without a dot |
| `rules.match.encrypted` | Whether the input ZIP entry was encrypted |
| `rules.match.duplicate` | Whether the payload content has duplicate occurrences |
| `rules.match.min_size`, `max_size` | Inclusive byte bounds; readable sizes or integers; numeric zero is allowed |
| `rules.output.directory`, `filename` | Destination templates overriding organization defaults |

Matchers in one rule must all match. Template fields include `file_type`,
`original_extension`, `original_name`, `original_stem`, `sha256`, `size`,
`encrypted`, `duplicate`, `source_archive`, `ingest_date`, and `occurrence_id`.
Archive-derived records require `.sample` filenames. Paths are validated and
collisions resolved by the existing planner.
