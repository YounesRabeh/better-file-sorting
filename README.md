<p align="center">
  <a href="https://github.com/younidev-org/better-file-sorting">
    <img src="assets/better-file-sorting-banner.svg" alt="Better File Sorting — Your files. Your rules." width="100%" />
  </a>
</p>

<div align="center">

  <p>
    <img src="https://img.shields.io/badge/Platform-Linux-1793D1?style=for-the-badge&amp;logo=linux&amp;logoColor=white" alt="Supported platform: Linux">
    <a href="https://github.com/younidev-org/better-file-sorting/releases"><img src="https://img.shields.io/badge/Release-View_Releases-2EA44F?style=for-the-badge&amp;logo=github&amp;logoColor=white" alt="View releases"></a>
    <a href="LICENSE"><img src="https://img.shields.io/badge/License-GPL_3.0_or_later-2EA44F?style=for-the-badge" alt="License: GNU GPL v3.0 or later"></a>
    <a href="docs/README.md"><img src="https://img.shields.io/badge/Documentation%20Hub-0969DA?style=for-the-badge&amp;logo=mdbook&amp;logoColor=white" alt="Open the documentation hub"></a>
  </p>

  <p>A command-line tool for sorting files and ZIP collections with configurable policies, deduplication, and optional encrypted ZIP output.</p>

  <p>
    <a href="#features">Features</a> •
    <a href="#showcase">Showcase</a> •
    <a href="#quick-start">Quick Start</a> •
    <a href="#usage">Usage</a> •
    <a href="#development">Development</a> •
    <a href="#guides">Guides</a>
  </p>
</div>

---

- 📁 Sort ordinary files and folders, or extract entries from ZIP collections.
- 📋 Define matching rules, destination paths, filenames, and duplicate handling in TOML.
- 🔗 Deduplicate content with SHA-256 while retaining source metadata.
- 🔍 Preview the final layout, including filename collisions and duplicate counts.
- 📦 Export a compressed ZIP with optional AES-256 encryption and size-savings reports.
- 🛡️ Apply private permissions, bounded extraction, and cooperative cancellation.

```mermaid
flowchart LR
    Files[Files and folders] --> Pipeline[Classify and deduplicate]
    ZIP[ZIP archive contents] --> Pipeline
    Rules[TOML policies] --> Layout[Organized layout]
    Pipeline --> Layout
    Layout --> Repository[Repository and reports]
    Layout --> Export[Optional ZIP output]
```

## Detailed content detection

bfsort now stores precise formats and filename/content mismatch decisions alongside
its legacy categories. It recognizes executable, Office/ZIP package, archive,
image, database and strongly marked script formats. Reports work from metadata,
including sealed repositories; a mismatch is not a malware verdict.

```bash
bfsort report mismatches ./organized --format csv --output mismatches.csv
bfsort stats ./organized --json
bfsort export ./standard-repository --rules examples/rules.formats.example.toml --zip-each-dir -o ./by-format
```

See [detection, compatibility and export guide](docs/detection.md) for supported
formats, bounded inspection, explicit legacy analysis, multiple independent
exports and limitations. Payload exports require a standard-mode repository.

## Showcase

One policy can group files by detected type and retain their original names.
Source files remain untouched; ZIP output contains the selected files and a
`layout.jsonl` index.

<details>
<summary><strong>View a regular-file policy and example output</strong></summary>

```toml
version = 1

[organization]
filename_policy = "regular"
duplicates = "first_only"
default_directory = "{file_type}"
default_filename = "{original_name}"
```

For a folder containing PDF documents and text files, the ZIP layout can look like:

```text
sorted.zip
├── pdf/
│   └── invoice.pdf
├── text/
│   └── notes.txt
└── layout.jsonl
```

The first occurrence of identical content is kept. Conflicting filenames receive
an occurrence identifier before the extension. Classification uses file contents;
it is not based only on extensions.

</details>

## Quick start

Download a Linux executable from [Releases](https://github.com/younidev-org/better-file-sorting/releases),
or build from source with stable Rust and Rust 2024 support:

```sh
cargo build --release --locked
```

The examples below use `bfsort` on your `PATH`. For a local build, replace `bfsort`
with `target/release/bfsort`.

Copy the regular-file policy, then preview and export a folder:

```sh
cp examples/rules.files.example.toml rules.toml

bfsort ingest ./documents --input-mode files --rules ./rules.toml \
  --output ./sorted.zip --zip --dry-run

bfsort ingest ./documents --input-mode files --rules ./rules.toml \
  --output ./sorted.zip --zip
```

The preview hashes and classifies inputs in sealed temporary storage. It prints
planned destinations, source paths, and duplicate counts without publishing
output. The export refuses existing ZIP destinations and failed or interrupted
runs. Already-compressed files may shrink little or produce a larger ZIP.

## Download collections

For the default workflow, no configuration file is required:

```sh
bfsort collect https://example.org/downloads/ --dry-run
bfsort collect https://example.org/downloads/
# Explicitly unlock the completed receipt-owned payloads before extraction.
bfsort ingest --extract
bfsort stats
```

Downloads use three workers with no pacing delay or speed, request-count, file-count,
or byte caps. Network timeouts, sealed permissions and extraction safeguards remain
active. `bfsort ingest` uses completed default download receipts, or `./incoming` when
none are configured/present; output defaults to `./organized`. `bfsort inspect` reads
`./incoming`; `bfsort reorganize --dry-run` previews default rules for `./organized`.

Use [examples/collection.example.toml](examples/collection.example.toml) to discover direct ZIP links
on one HTTP(S) page, preview selection, and download into sealed private staging:

```sh
bfsort discover --config collection.toml
bfsort download --config collection.toml --dry-run
bfsort download --config collection.toml
```

> [!NOTE]
> Downloads have `0000` permissions. Explicitly unlock all receipt-owned ZIPs
> before ingestion, using their paths in the download report/control receipts:

```sh
chmod 600 ./collection/downloads/payloads/FILE.zip
bfsort ingest --config collection.toml --dry-run
bfsort ingest --config collection.toml
```

Ingest with no explicit input paths verifies completed receipts and hashes before
processing. Explicit local inputs keep their existing behavior. Completed downloads
are retained; bfsort never automatically unseals or deletes them. After unlocking,
`download` intentionally refuses reuse until you explicitly reseal the files
with `chmod 000`. Sealing is a file-permission safeguard, not a sandbox.

Policies match original filenames case-insensitively and choose
which ZIPs to download and stage; existing sorting rules organize their contents.
See [acquisition configuration](docs/commands.md#acquisition-commands) for limits.

## Usage

CLI options override [runtime configuration](examples/config.example.toml). Sorting rules
are shared across repository and ZIP output. See the [command reference](docs/commands.md)
for every argument, default, report field, and exit code.

### Files, archives, and repositories

Use `--extract` for archive contents. Archive input mode searches
for ZIP archives and extracts their entries; ZIPs are treated as ordinary files
when file mode is selected.

<details>
<summary><strong>Commands</strong></summary>

| Task | Command |
| --- | --- |
| Show all ingest options | `bfsort ingest --help` |
| Sort ordinary files into a readable private repository | `bfsort ingest ./documents --input-mode files --rules ./examples/rules.files.example.toml --output ./organized --security-mode standard` |
| Inspect ZIP metadata without extracting contents | `bfsort inspect ./incoming` |
| Ingest ZIP entries into a sealed repository | `bfsort ingest ./incoming --extract --rules ./examples/rules.example.toml --output ./samples` |
| Preview destinations before ingest | `bfsort ingest ./incoming --extract --rules ./examples/rules.example.toml --output ./samples --dry-run` |
| Show repository statistics | `bfsort stats ./organized` |
| Preview new rules for an existing repository | `bfsort reorganize ./organized --rules ./rules.toml --dry-run` |
| Publish a new view with updated rules | `bfsort reorganize ./organized --rules ./rules.toml` |
| Limit discovery to immediate children | `bfsort ingest ./documents --input-mode files --recursive false --output ./organized` |
| Load runtime settings from TOML | `bfsort ingest --config ./config.toml` |

</details>

The default filename policy requires `.sample` filenames. The
[regular-file policy](examples/rules.files.example.toml) preserves ordinary names and
extensions. Archive inputs and archive-derived records refuse the regular
policy; older metadata without input-mode provenance is treated as archive-derived.
Empty directories are not copied.

Repositories store deduplicated objects, source metadata, reports, and logical
views. Reorganization uses metadata without unlocking payloads, and retains
previous views. A ZIP export is an output layout, not a repository backup.

### ZIP output and passwords

`--password-file` decrypts inputs; `--output-password-file` encrypts output with
AES-256. Both require `0600` permissions; the output password file must contain
exactly one nonempty line. Password files and their hard links are excluded from inputs.

<details>
<summary><strong>Commands</strong></summary>

| Task | Command |
| --- | --- |
| Sort ordinary files into a ZIP | `bfsort ingest ./documents --input-mode files --rules ./examples/rules.files.example.toml --zip --output ./sorted.zip` |
| ZIP selected sorted directories together | `bfsort ingest ./documents --input-mode files --rules ./examples/rules.files.example.toml --zip-dir documents/pdf --zip-dir text --output ./selected.zip` |
| Create one ZIP per top-level sorted directory | `bfsort ingest ./documents --input-mode files --rules ./examples/rules.files.example.toml --zip-each-dir --output ./exports` |
| Create separate ZIPs for selected directories | `bfsort ingest ./documents --input-mode files --rules ./examples/rules.files.example.toml --zip-dir documents/pdf --zip-dir text --zip-each-dir --output ./exports` |
| Encrypt the output ZIP with AES-256 | `bfsort ingest ./documents --input-mode files --rules ./examples/rules.files.example.toml --zip --output ./private.zip --output-password-file ./output-password.txt` |
| Decrypt archive inputs and export their sorted contents | `bfsort ingest ./incoming --extract --password-file ./input-passwords.txt --rules ./examples/rules.example.toml --zip --output ./sorted.zip` |
| Select temporary storage for ZIP output | `bfsort ingest ./documents --input-mode files --rules ./examples/rules.files.example.toml --zip --output ./sorted.zip --staging-dir /existing/scratch` |

</details>

`--zip-dir` selects sorted directories and descendants; `--zip-each-dir` creates
separate ZIPs. Both enable ZIP output automatically. Separate exports preserve
hierarchy (`documents/pdf` → `exports/documents/pdf.zip`) in a new output directory.
Each ZIP includes a layout index. Preview with `--dry-run`; see the
[selection reference](docs/commands.md#selecting-sorted-directories).

<small><em>Reports show payload/output sizes, savings and elapsed time; negative
savings mean growth. Encryption leaves filenames visible and requires an
AES-compatible reader. Archive previews need input passwords; use
<code>inspect</code> for metadata only.</em></small>

### Permissions and temporary storage

| Setting | Behavior |
| --- | --- |
| `sealed` repository mode (default) | Payload permissions are `0000`; automatic unsealing is refused. |
| `standard` repository mode | Payloads are readable and writable by their owner (`0600`). |
| ZIP output | Uses a disposable private, readable repository and publishes a `0600` ZIP only after success. |
| Preview | Uses sealed disposable storage and includes existing destination metadata without changing its current view. |
| `--staging-dir <DIR>` | Creates a private `0700` working directory beneath an existing directory; normal repository ingestion keeps its internal staging location. |

ZIP staging defaults to the output directory; preview staging defaults to the
system temporary directory. ZIP mode requires space for uncompressed,
deduplicated inputs plus the compressed output. Final publication stays in the
output directory even when staging is on another filesystem.

> [!IMPORTANT]
> File permissions are not a malware sandbox. Use an isolated environment for
> untrusted samples. A forced termination or crash can leave private plaintext
> ZIP staging under `.bfs-zip-*`; select an encrypted filesystem when crash
> confidentiality is required. Normal completion and handled errors clean up
> temporary storage. Preview payloads remain sealed, though metadata can survive
> a crash.

## Development

Use stable Rust with Rustfmt and Clippy. Cargo Nextest is optional; install it
with `cargo install cargo-nextest --locked` to use the same test runner as CI.

<details>
<summary><strong>Build and verification commands</strong></summary>

| Task | Command |
| --- | --- |
| Build the release executable | `cargo build --release --locked` |
| Check formatting | `cargo fmt --all -- --check` |
| Apply formatting | `cargo fmt --all` |
| Run Clippy | `cargo clippy --locked --all-targets --all-features -- -D warnings` |
| Run tests and doctests with Cargo | `cargo test --locked` |
| Run tests with Nextest | `cargo nextest run --locked` |

</details>

## Guides

Choose a reference by task, or browse the [documentation hub](docs/README.md).

| I want to… | Start here |
| --- | --- |
| Look up commands, options, report fields, or exit codes | [Command reference](docs/commands.md) |
| Configure discovery, limits, workers, and staging | [Runtime configuration](examples/config.example.toml) |
| Customize sample sorting and duplicate handling | [Sample rules](examples/rules.example.toml) |
| Preserve ordinary filenames and extensions | [Regular-file rules](examples/rules.files.example.toml) |
| Read packaged release usage notes | [Release guide](docs/RELEASE_README.md) |
| Review validation evidence and limitations | [Validation record](docs/validation.md) |

## Tech stack

<p>
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/Rust-stable-CE422B?style=for-the-badge&amp;logo=rust&amp;logoColor=white" alt="Rust stable"></a>
  <a href="https://doc.rust-lang.org/edition-guide/rust-2024/"><img src="https://img.shields.io/badge/Edition-2024-000000?style=for-the-badge&amp;logo=rust&amp;logoColor=white" alt="Rust 2024 edition"></a>
  <img src="https://img.shields.io/badge/Policies-TOML-5C7568?style=for-the-badge" alt="TOML sorting policies">
  <a href="https://github.com/features/actions"><img src="https://img.shields.io/badge/GitHub_Actions-CI-2088FF?style=for-the-badge&amp;logo=githubactions&amp;logoColor=white" alt="GitHub Actions continuous integration"></a>
</p>

---

## License

Distributed under the [GNU GPL v3.0 or later](LICENSE).
