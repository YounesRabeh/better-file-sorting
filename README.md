<a href="https://github.com/younidev-org/better-file-sorting"><img src="assets/better-file-sorting-banner.svg" alt="Better File Sorting" width="100%"></a>

<div align="center">

  <p>
    <img src="https://img.shields.io/badge/Platform-Linux-f4c542?style=for-the-badge&amp;logo=linux&amp;logoColor=white" alt="Linux">
    <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/Rust-2024-000000?style=for-the-badge&amp;logo=rust&amp;logoColor=white" alt="Rust 2024"></a>
    <a href="https://github.com/younidev-org/better-file-sorting/releases"><img src="https://img.shields.io/badge/Release-View_Releases-2ea44f?style=for-the-badge" alt="View releases"></a>
    <a href="LICENSE"><img src="https://img.shields.io/badge/License-BSD--3--Clause-2ea44f?style=for-the-badge" alt="BSD 3-Clause License"></a>
    <a href="docs/README.md"><img src="https://img.shields.io/badge/Documentation_Hub-0969DA?style=for-the-badge&amp;logo=mdbook&amp;logoColor=white" alt="Documentation hub"></a>
  </p>

  <p>A CLI that turns sample collections into organized repositories.</p>

  <p>
    <a href="#features">Features</a> •
    <a href="#quick-start">Quick Start</a> •
    <a href="#development">Development</a> •
    <a href="#guides">Guides</a>
  </p>
</div>

---

## Features

- **Ingest** unencrypted, ZipCrypto, and AES ZIP entries without executing samples.
- **Organize** by metadata rules, with SHA-256 deduplication and file classification.
- **Sort folders** without modifying sources; optionally publish only a compressed ZIP with AES-256 password protection.
- **Inspect** duplicates, failures, and storage usage through JSON reports.
- **Restrict access** with Unix permissions and sealed storage by default.

```mermaid
flowchart LR
    ZIP[ZIP archives] --> Objects[Deduplicated objects]
    Objects --> Views[Rule-based views]
    Objects --> Reports[Reports]
```

> [!CAUTION]
> Sealed permissions are not a malware sandbox. Use an isolated environment and keep password files private (`0600`). Never commit samples or credentials.

## Quick start

Requires Linux and stable Rust with Rust 2024 support. The CLI executable is `bfs`.

### 1. Build and configure

```sh
cargo build --release
cp config.example.toml config.toml
cp rules.example.toml rules.toml
```

### 2. Inspect and ingest

```sh
target/release/bfs inspect ./downloads
target/release/bfs ingest ./downloads \
  --output ./repository --config ./config.toml --rules ./rules.toml
target/release/bfs stats ./repository
```

### 3. Preview a new layout

```sh
target/release/bfs reorganize ./repository --rules ./rules.toml --dry-run
```

### Sort regular folders and ZIP the result

```sh
# Sort ordinary files (ZIPs are treated as files rather than extracted).
target/release/bfs ingest ./documents --input-mode files \
  --output ./repository --rules ./rules.files.example.toml --security-mode standard

# Keep only the sorted ZIP output, without a permanent repository.
target/release/bfs ingest ./documents --input-mode files --zip \
  --output ./sorted.zip --rules ./rules.files.example.toml

# Optional AES-256 password: one nonempty line in a private (0600) file.
target/release/bfs ingest ./documents --input-mode files --zip \
  --output ./private.zip --rules ./rules.files.example.toml \
  --output-password-file ./output-password.txt
```

`--zip` also works with the default archive input mode: extract inputs, sort
their entries, then compress the resulting layout. `--password-file` supplies
input decryption candidates; `--output-password-file` independently sets the
output password. Both password files, including hard links, are excluded from
discovered inputs.

Sorting policy stays in one rules TOML: ordered matchers, directory/filename
templates and duplicate handling apply to both input modes and output formats.
The default `filename_policy = "samples"` requires `.sample` filenames. For
ordinary files, [rules.files.example.toml](rules.files.example.toml) sets
`filename_policy = "regular"` and `default_filename = "{original_name}"` to keep
extensions; collision identifiers are inserted before the extension. The
regular policy is refused for archive inputs and repositories containing
archive-derived records. Older metadata without input-mode provenance is
conservatively treated as archive-derived. In file mode,
`source_archive` identifies the source file and `encrypted` is false.
Use `--recursive false` for immediate children only. `--dry-run` hashes and
classifies inputs in sealed temporary storage, then prints the actual proposed
layout, source paths and duplicate counts as JSON. It includes existing
repository metadata without publishing or changing its current view. Archive
previews read/decompress entries and require input passwords; use `inspect` for
metadata-only inspection. Empty directories are not copied.

Sources are never moved or deleted. Repository output retains the configured
security mode (sealed by default). ZIP output uses a disposable private,
readable repository, never unlocks existing sealed storage, and is published
as a `0600` file only after a successful run. It refuses existing output paths
and partial/failed runs. The ZIP includes policy-selected payloads and
`layout.jsonl`, even with `view_mode = "manifest"`; it is not a repository
backup. ZIP filenames remain visible when contents are encrypted; extraction
requires an AES-compatible reader.

Set `--staging-dir /path/to/scratch` (or `staging_dir` in config) to choose an
existing directory for private ZIP/preview working storage. ZIP staging defaults
to the output parent, preview staging to the system temporary directory. A
private `0700` child is created; the selected parent is not modified or cleared.
Final ZIP publication stays in the output directory so it remains atomic even
when staging is on another filesystem. Repository ingestion uses its existing
internal `.staging` directory.

An uncatchable kill or crash may leave private plaintext ZIP staging under
`.bfs-zip-*` in the selected location; use an encrypted filesystem when crash
confidentiality is required. Preview payloads remain sealed, though temporary
metadata can survive a crash. Normal completion and handled errors remove
temporary storage.

ZIP reports add a `zip` object with selected `payload_bytes`, actual
`output_bytes`, signed `saved_bytes`, `savings_percent` and `ingest_seconds`.
`elapsed_seconds` includes the full ZIP run and cleanup. Savings compare ZIP
size (including its layout/index overhead) with policy-selected payload bytes;
negative values mean the ZIP grew. A zero-byte payload has a `null` percentage.
Existing `summary` storage fields describe the temporary ingest repository.
If an input fails, ZIP errors include its source and cause, and no ZIP is
published; repository reports also retain `first_failure`.

ZIP mode needs temporary space for the uncompressed, deduplicated inputs plus
the compressed output. Already-compressed files may shrink little. File count
is bounded by `max_entries`, and each ordinary file by the smallest of
`max_entry_bytes`, `max_input_bytes` and `max_archive_bytes`.

## Development

| Need | Command |
| --- | --- |
| Check formatting | `cargo fmt --all -- --check` |
| Run Clippy | `cargo clippy --locked --all-targets --all-features -- -D warnings` |
| Run tests | `cargo nextest run --locked` |

## Guides

Choose a reference by task, or browse the [documentation hub](docs/README.md).

| I want to… | Start here |
| --- | --- |
| Look up commands, options and examples | [Command reference](docs/commands.md) |
| Configure ingestion and limits | [Configuration template](config.example.toml) |
| Customize sorting and duplicates | [Organization rules](rules.example.toml) |
| Preserve ordinary filenames and extensions | [Regular-file rules](rules.files.example.toml) |
| Understand release usage and safety | [Release guide](docs/RELEASE_README.md) |
| Review tests and known limitations | [Validation record](docs/validation.md) |

---

## License

Distributed under the [BSD 3-Clause](LICENSE).
