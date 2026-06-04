# ttk — Terminal Tool Kit

A single binary with 10 focused commands for everyday dev tasks: file operations, image/audio/video processing, git digests, and build signing.

Built in Rust. No runtime required.

---

## Install

**From source (requires Rust):**

```bash
git clone https://github.com/arielonoriaga/term-toolkit.git
cd term-toolkit
cargo build --release
# binary at target/release/ttk
```

**Add to PATH:**

```bash
ln -sf "$(pwd)/target/release/ttk" ~/.local/bin/ttk
```

---

## Commands

### `ttk deleter <directory>`

Delete files by position index (even or odd).

```bash
ttk deleter ./exports          # deletes odd-indexed files (default)
ttk deleter ./exports --even   # deletes even-indexed files (0, 2, 4...)
```

---

### `ttk rename <new-name>`

Rename all files in a directory to a sequential name with zero-padded index.

```bash
ttk rename photo -d ./vacation
# → photo0.jpg, photo1.jpg, photo2.jpg ...
```

Files are sorted alphabetically before renaming. Extension is preserved.

---

### `ttk optimize <directory>`

Compress images in place or to an output directory. Supports JPEG (quality-aware), PNG, WebP, GIF, TIFF.

```bash
ttk optimize ./images
ttk optimize ./images -q 60 -o ./compressed
ttk optimize ./images --keep-original=false   # overwrite originals
```

| Flag | Default | Description |
|---|---|---|
| `-q, --quality` | `80` | Quality 1–100 |
| `-o, --output` | same dir | Output directory |
| `--keep-original` | `true` | Keep original file |

---

### `ttk clone-repo <url>`

Clone a repository and optionally reset its git history to a single initial commit.

```bash
ttk clone-repo https://github.com/user/repo.git
ttk clone-repo https://github.com/user/repo.git -o my-project
ttk clone-repo https://github.com/user/repo.git --reset=false  # keep history
```

---

### `ttk git-digest <directory>`

Scan all git repositories under a directory, collect commits in a date range, filter by author, and output a colored terminal summary + Markdown report.

```bash
ttk git-digest ~/projects --last 7d
ttk git-digest ~/projects --since 2026-01-01 --until 2026-01-31
ttk git-digest ~/projects --last 24h --output-dir ~/reports
```

| Flag | Description |
|---|---|
| `--last 24h\|7d\|30d` | Relative range (skips interactive wizard) |
| `--since DATE` | Start date (ISO 8601) |
| `--until DATE` | End date (ISO 8601, defaults to today) |
| `--output-dir` | Directory for the `.md` report |

When no flags are given, an interactive wizard prompts for date range and authors.

---

### `ttk sign <folder> <prefix>`

Generate MD5 and SHA1 checksum files for all files in a folder. Output is compatible with `md5sum -c` / `sha1sum -c`.

```bash
ttk sign ./dist my-release
# → my-release.md5, my-release.sha1
```

---

### `ttk copy-clean <source> <destination>`

Copy a project folder, stripping dev artifacts (`node_modules`, `.git`, `.github`, `dist`, `*.md5`, `*.sha1`, `*.zip`).

```bash
ttk copy-clean ./my-app ./my-app-clean
```

---

### `ttk build-and-sign <source> <prefix>`

Combine `copy-clean` + `sign` + zip in one step. Creates a distributable archive with integrity checksums.

```bash
ttk build-and-sign ./my-app release-v1.0
# → release-v1.0.zip, release-v1.0.md5, release-v1.0.sha1
```

---

### `ttk mp3-compress [input] [output-dir]`

Convert MP3 files to M4A (AAC, 96k). Requires `ffmpeg`.

```bash
ttk mp3-compress                          # all *.mp3 in current dir
ttk mp3-compress ./music ./compressed
ttk mp3-compress track.mp3 ./out
ttk mp3-compress track.mp3 ./out --stereo # keep stereo (default: mono)
```

---

### `ttk mp4-optimize [input] [output-dir] [--quality]`

Optimize MP4 for web streaming and iOS compatibility (H.264, AAC-LC, faststart). Requires `ffmpeg`.

```bash
ttk mp4-optimize                           # current dir, web quality
ttk mp4-optimize ./videos ./out --quality high
ttk mp4-optimize clip.mp4 ./out --quality medium
```

| Preset | CRF | Max res | Notes |
|---|---|---|---|
| `web` (default) | 25 | 1280×720 | iOS-compatible, baseline profile |
| `low` | 28 | 854×480 | Smallest files |
| `medium` | 23 | 1280×720 | Balanced |
| `high` | 18 | 1920×1080 | Best quality, high profile |

Output files are named `{original}_optimized.mp4`.

---

### `ttk mutate <paths>... --test "<cmd>"`

Language-agnostic mutation testing. Mutates source text (operator swaps), runs your test command, and reports which mutants survived — i.e. where your tests are blind.

```bash
ttk mutate src --test "cargo test"
ttk mutate src --test "pytest -q" --build "python -m compileall -q src"
ttk mutate . --test "npm test" --since HEAD~5 --jobs 8
```

- `--test` (required): command that exits 0 when healthy. A mutant that keeps it green is a survivor (test gap).
- `--build` (optional): compile step run before the test; failure marks the mutant *unviable* instead of *killed*. Without it, compile-broken mutants count as killed.
- `--since <ref>`: only mutate lines changed since a git ref (great for CI).
- `--max-mutants <N>`, `--jobs <N>`, `--retest <N>`, `--timeout <secs>`, `--include`/`--exclude` globs, `--config <file>`, `--report <file.md>`.

Extend the rule table per repo with `.ttk-mutate.toml`:

```toml
[[rule]]
find = "and"
replace = "or"
```

---

## Requirements

| Command | Dependency |
|---|---|
| All | Rust (build only) |
| `mp3-compress` | `ffmpeg` |
| `mp4-optimize` | `ffmpeg` |

---

## License

MIT
