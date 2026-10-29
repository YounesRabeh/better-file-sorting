# Malware Organizer

A Rust 2024 CLI that streams ZIP contents into permission-restricted, content-addressed storage, then builds deterministic views from metadata. It never executes samples, invokes external extraction tools, or recursively extracts nested archives.

**Pre-release:** Unix storage is implemented; Windows inspection works, but storage fails closed until a Windows ACL backend is implemented. Permission restrictions are not a malware sandbox. Version 1.0 has not been declared.

## Build and use

Install stable Rust and run `cargo build --release`. No elevated privileges are needed.

```sh
malware-organizer inspect ./downloads
malware-organizer ingest ./downloads --output ./repository --config ./config.toml --rules ./rules.toml --workers 4
malware-organizer stats ./repository
malware-organizer reorganize ./repository --rules ./rules.toml --dry-run
malware-organizer reorganize ./repository --rules ./rules.toml
```

Copy `config.example.toml` and `rules.example.toml` into local configuration files. Configure a private `0600` password file with one candidate per line; blank lines are ignored, whitespace is otherwise significant. Passwords are never command-line values or logged. Password files are limited to 1 MiB and 256 candidates. The output's parent directory must already exist. Existing repository directories must be `0700`; the application refuses to repair permissions implicitly.

Explicit CLI options override the config file, which overrides defaults. Inputs supplied on the command line replace configured inputs. Relative config paths resolve against the working directory. Unknown fields and unsupported values are errors. Use `--recursive false` to override recursive traversal. `--security-mode standard` overrides the default `sealed` mode for a **new** repository; an existing repository's mode cannot be changed automatically.

ZIP discovery uses signatures rather than extensions and skips output directories and symlinks. Stored and Deflate entries support unencrypted, ZipCrypto, and AES inputs, including different passwords per entry. Other compression methods are reported as unsupported. Self-extracting executables and split ZIPs are not discovered/supported. Nested ZIPs are stored as ordinary samples.

`inspect` reads archive metadata only. `ingest --dry-run` validates configuration/rules and previews archive metadata without creating a repository; detected content types, hashes, duplicates and final names require ingestion. Use `reorganize --dry-run` to preview the exact layout of an existing repository without modifications. JSON goes to stdout, progress and human summaries to stderr. Exit codes: **0** success, **2** partial success/interruption with accepted samples, **1** failure (clap uses **2** for invalid CLI syntax).

## Storage, security and recovery

```text
repository/
  security.json                 fixed repository security mode
  objects/<hash-prefix>/<hash>   one payload per SHA-256
  manifests/<run-id>.jsonl       append-only occurrence events
  reports/<run-id>.jsonl         per-entry/archive failures
  reports/<run-id>.json          summary and throughput
  views/CURRENT                 active generation name
  views/view-*/layout.jsonl      logical paths and canonical hashes
  views/view-*/.../*.sample      hard links, when enabled
  .staging/                     private temporary work
```

Unix canonical payload permissions are `0600` in standard mode and `0000` in sealed mode. Metadata and lock files are `0600`; repository and view directories are `0700`. A hard link shares its canonical object's permissions and bytes. Existing objects are validated, never automatically unlocked or overwritten. Original archive names never become extraction paths.

Staging payloads are created with `0000` permissions. The initial writable descriptor permits streaming, hashing and classification without reopening the path; final policy is applied only after integrity validation, and the writer is closed before atomic publication. This makes even a forced-stop leftover inaccessible through ordinary file opens. Graceful Ctrl-C removes incomplete files. On the next ingestion, an exclusive repository lock permits removal of abandoned application staging directories. Committed objects without a corresponding manifest record are retained and reported as orphans; re-ingesting the unchanged source repairs provenance. No automatic payload deletion occurs.

A newline-terminated manifest record is synchronized before acknowledging acceptance. An unterminated final record from interruption is ignored with a warning; malformed complete records fail closed. Identical source path, entry index and content have one logical occurrence across repeated ingestion; all run events remain in manifests. Distinct archive paths and entry indexes retain separate provenance. Non-UTF-8 source paths use an `encoded-path:` hexadecimal representation of platform path bytes.

Reorganization builds a complete private generation, synchronizes it, publishes it and atomically replaces `views/CURRENT`. Previous generations remain for recovery; they are not silently deleted. A failure before switching leaves the original pointer intact. Failures after generation publication can leave an unused generation. Directory synchronization improves crash durability, but power-loss behavior depends on the filesystem and hardware and has not been fault-injection tested.

The owner can change permissions; root/admin and processes already holding file descriptors can bypass ordinary access restrictions. Same-account hostile processes that replace filesystem components concurrently are outside this permission boundary. Private repository directories and symlink checks protect against static path redirection; this is not a defense against a compromised processing account. Unix ACLs, network filesystems and unusual permission semantics require host validation. No Windows read-only flag is presented as sealing.

Use a dedicated unprivileged account, isolated VM/container, read-only source mounts and filesystem quotas. `noexec,nosuid,nodev` mounts help, but `noexec` does not prevent an interpreter from reading scripts. Output policy does not secure original archives or password files elsewhere.

## Rules

Rules are first-match-wins. All configured match fields must match; omitted fields match anything. `organization.default_directory` and `default_filename` form the required fallback and have safe defaults. An unconditional rule shadows all later rules.

Matchers: `file_type`, case-insensitive `original_extension`, `encrypted`, `duplicate`, inclusive `min_size`, inclusive `max_size`. Detected types: `pe`, `elf`, `macho`, `pdf`, `zip`, `text`, `other`, `unknown`. Classification uses at most the first 8 KiB; text detection and magic signatures are heuristics, not validation of a complete format.

Templates expose only these placeholders:

`file_type`, `original_name`, `original_stem`, `original_extension`, `sha256`, `size`, `encrypted`, `duplicate`, `source_archive`, `ingest_date`, `occurrence_id`.

No expressions, shell commands or user code are evaluated. Metadata substitutions are Unicode NFC normalized, separators/control characters are replaced, Windows reserved names are prefixed, and each component is bounded to 120 UTF-8 bytes before collision suffixes. Final components remain below 255 bytes. Directory templates have at most eight components; full relative paths are bounded to 2048 bytes. Absolute/traversal paths are rejected. Directory names are lowercased; filename collisions are detected case-insensitively and receive a stable occurrence-ID suffix. File/directory conflicts and the reserved root `layout.jsonl` path are rejected during planning. All generated payload filenames require `.sample`, in both modes.

Duplicate policies:

| Value | View behavior |
|---|---|
| `first_only` | One representative per hash, ordered by source path and entry index |
| `all` | Every source occurrence |
| `only_duplicates` | Every occurrence of hashes occurring more than once |
| `separate` | First occurrence in the normal location, later occurrences under `duplicates/` |

Canonical payloads remain deduplicated under every policy. Duplicate matching uses the complete metadata collection, independent of worker completion order. Ingestion dates use UTC. `view_mode = "manifest"` creates only `layout.jsonl`; `hardlink` fails clearly on unsupported filesystems and never silently copies payload bytes.

## Limits and reporting

Workers process entries sequentially within an archive. Each active worker uses a 64 KiB streaming buffer, an 8 KiB classification prefix and at most one staged payload. Actual decompressed bytes, including failed password attempts that reach decompression, count against archive limits. ZIP CRC/AES authentication is checked by reading through EOF before publication. Entry count, input size, expanded size and compression ratio are validated; size limits are not trusted solely from ZIP metadata.

Archive discovery and organization retain paths/metadata proportional to collection size. The ZIP library parses the central directory before the entry-count check; the compressed-input byte limit bounds this input, but is not an exact RAM cap. Use OS memory/disk quotas for adversarial collections. Concurrent archives may each consume their configured entry limit. This implementation does not claim constant total memory independent of collection size.

Statistics read metadata and filesystem attributes, never payload contents. Reports include logical occurrences, unique manifested hashes, duplicate occurrences, physical canonical object counts, orphan counts, logical/stored/allocated payload bytes and category counts. Allocated bytes exclude metadata, archive inputs and directory overhead. Run reports include accepted entries, failures, elapsed seconds and bytes/second. Sealed content is never rehashed automatically.

## Development and verification

Tests generate harmless synthetic content only. Never place real samples or credentials in this repository.

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo nextest run --locked
cargo test --locked --doc
cargo build --locked --release
cargo audit
cargo +nightly fuzz run untrusted_inputs -- -max_total_time=30 -max_len=65536
cargo bench --bench throughput
```

`cargo test` runs the complete functional suite when nextest is unavailable. Run permission-denial tests as an ordinary user; root access is not representative. CI covers Linux, macOS and Windows, with storage tests enabled only on Unix and explicit Windows fail-closed coverage. ZIP is pinned because synthetic ZipCrypto fixture generation uses its documented unstable writer API. No production encryption is implemented locally.

The benchmark is a dependency-free, end-to-end harness with correctness assertions and JSONL results. It measures 10,000 small archives, 100 compressible and 100 incompressible 1 MiB archives, one archive with 10,000 entries, high duplication, metadata loading, and 100-rule reorganization; every scenario uses 1/2/4/8/16 workers and both modes. `MO_BENCH_QUICK=1 cargo bench --bench throughput` runs a smaller smoke matrix. Use an external resource meter for CPU and peak RSS. See `docs/validation.md` for measured results and release limitations.
