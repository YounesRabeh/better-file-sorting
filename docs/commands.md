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
| `--rules <FILE>` | Load the central sorting policy: ordered matchers, templates and duplicate handling. | Built-in rules: first occurrence per content hash, grouped by file type, named `{sha256}.sample`. |
| `--zip` | Enable ZIP-only output. Refuses existing destinations and failed or interrupted runs. | Disabled. Can also be enabled with `zip = true` in config. |
| `--output-password-file <FILE>` | Encrypt output contents with AES-256. The file must have `0600` permissions and contain exactly one nonempty password line. | No output encryption. Requires ZIP output. |
| `--password-file <FILE>` | Read candidate passwords for encrypted input archives, one per nonempty line, from a `0600` file. Independent of the output password. | No candidates. |
| `--workers <N>` | Set parallel ingest workers. | Available CPU parallelism capped at 8; accepts `1`–`256`. |
| `--security-mode <MODE>` | Set repository payload permissions. Existing repository modes cannot be changed automatically. ZIP output always uses disposable private, standard-mode storage. | `sealed` (default, `0000` payloads), `standard` (`0600` payloads). |
| `--recursive <BOOL>` | Include nested folders or only immediate children. Pass the boolean explicitly. | `true` (default), `false`. |
| `--dry-run` | Write no output: inspect archive metadata in archive mode, or list discovered paths in file mode. | Disabled. This does not perform a full ingest or guarantee that a real run will succeed. |
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

## Password-protected ZIP example

Create a local password file with `0600` permissions, then run:

```sh
bfs ingest ./documents --input-mode files --rules ./rules.toml \
  --zip --output ./private.zip --output-password-file ./output-password.txt
```

Sources and password files remain untouched. Password files and their hard links
are excluded from discovered inputs. ZIP filenames remain visible even when
contents are encrypted; extraction requires an AES-compatible reader.

Policies are configured in [rules.example.toml](../rules.example.toml); runtime
settings and size/count limits are in [config.example.toml](../config.example.toml).
Output filenames must end in `.sample`. Use `{original_name}.sample` in a
filename template to retain readable original names.

ZIP mode needs temporary disk space for uncompressed, deduplicated inputs plus
the ZIP. Normal completion and handled errors remove staging; an uncatchable
kill or crash may leave private plaintext under `.bfs-zip-*` beside the output.
See the [ZIP usage notes](../README.md#sort-regular-folders-and-zip-the-result).

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Command succeeded. |
| `1` | Runtime error, or ingest failed/interrupted without accepting any files. ZIP publication failures return this code. |
| `2` | Repository ingest accepted some files but also encountered failures or interruption; CLI argument errors also use this code. |
