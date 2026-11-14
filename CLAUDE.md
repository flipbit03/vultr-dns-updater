# vultr-dns-updater — Development Guide

Dynamic DNS for Vultr DNS. The binary is `vultr-dns`; the crate is `vultr-dns-updater`. A Rust rewrite (v1.0.0) of the original Python tool (v0.1.0, `c67ec2d` in git history), built to be a drop-in replacement: same commands, flags, config file, systemd units and journal lines.

## Workflow

**Never commit directly to main.** Always create a branch and open a PR.

**After every change**, re-check and update documentation — `README.md` (the user-facing docs), `src/usage.rs` and this `CLAUDE.md`. Keep them in sync with the code.

## Quick Reference

```bash
cargo build                  # Build
cargo test                   # Run tests (offline)
cargo clippy --all-targets --all-features -- -D warnings   # Lint (warnings are errors)
cargo fmt --check            # Check formatting
RUSTDOCFLAGS=-Dwarnings cargo doc --all-features --no-deps # Doc lints
cargo install --path .       # Install locally for testing
```

`make check && make test` runs all of the above. Before pushing, always verify it passes.

## Architecture

Single binary crate. No workspace, no sub-crates.

```
src/
├── main.rs           # Clap CLI, dispatch, `Error: {:#}` + exit 1
├── config.rs         # Search order, load, validate, the example text, init-config, the API key (Secret)
├── http.rs           # The reqwest client builder (timeouts, User-Agent, IPv4-only for IP checks)
├── ip.rs             # Public IPv4 detection
├── vultr.rs          # Vultr API: types, paginated lists, create / patch, error mapping
├── sync.rs           # decide() (pure) + applying it; drives `update` and `status`
├── output.rs         # Info/Success/Warning/Error lines, tables, --format json
├── service.rs        # `service install|uninstall|status` (unit texts + sudo systemctl)
├── usage.rs          # `vultr-dns usage`: agent-facing reference (plain text)
├── self_update.rs    # `self update`: verified binary swap, or cargo install
└── version_check.rs  # GitHub latest release, 24h cache, semver compare, asset URLs
tests/
├── cli.rs            # The real binary against wiremock (Vultr + IP services)
├── usage.rs          # usage size cap, works with no config, --version shape
└── fixtures/         # my_infra's systemd unit files (see "systemd units")
```

## Key Design Decisions

**Drop-in output.** `update`'s lines are the Python tool's, word for word, blank lines included, because `journalctl -u vultr-dns-updater` is where they are read. `tests/cli.rs` holds a golden test for the "already up to date" run. Change a line only on purpose, and update the golden test with it. Warnings and errors go to stderr (stdout stays clean for `--format json`); colour only on a terminal and never with `NO_COLOR`.

**Never `println!`.** All output goes through `output.rs`'s `outln!` / `out!` / `errln!`, which ignore write errors: `vultr-dns usage | head` exits quietly, and an `update` whose stdout goes away (a restarted journald) still finishes its DNS work. SIGPIPE stays ignored (systemd's `IgnoreSIGPIPE=yes` too); don't reset it.

**One decision function.** `sync::decide` says what a target needs (create / update / up to date) from its record, the IP, the TTL and `--force`. `update` and `status` both use it, so "Needs update" means exactly what `update` would do, TTL included.

**Every target, every page.** Each domain's records are listed once per run, following `meta.links.next` with `per_page=500`; a cursor seen twice is an error, never a partial list (a target missing from a partial list would be created again). Writes update that cached list. A failing target prints its `Error:` line and the rest still run; the exit code is 1 if any failed.

**Names are case-insensitive.** Record matching ignores ASCII case (`Home` finds `home`), and the config refuses two targets naming the same record.

**No network on the timer path.** `update` never checks for updates. The cached "update available" hint is read (never fetched) by `usage` and `--help` only; `self update` is the only thing that calls GitHub.

**Self update is verified.** The asset's `.sha256` must exist and match; the new binary must print `vultr-dns <version>` for `--version`; the current binary is hard-linked to `vultr-dns.old`; then an atomic rename. A non-writable install dir fails before downloading, with a generic message (the binary doesn't know how it was installed). A dev build (`0.0.0`) refuses.

**TLS.** reqwest 0.12 with `rustls-tls`: rustls on `ring` with bundled webpki roots, as in tu and lineark. Nothing from the OS at runtime. Bundled roots age: run `cargo update` before a release.

**The API key** is a `config::Secret`, whose `Debug` prints `***`. It never appears in a message, and `--api-key`'s env value is hidden from `--help`. Config parse errors are built from toml's message and position only, never its `Display` (which quotes the source line, key included), and serde's `string "<value>"` is elided; `api_key` and `ttl` are read as raw TOML values so a type error can't echo them. Precedence: `--api-key` > `VULTR_API_KEY` > `api_key` in the file.

**Config.** The Python file format, unchanged. `api_key` is optional in the file. Unknown keys warn (`serde_ignored`). Search order: `--config`, `$XDG_CONFIG_HOME/vultr-dns-updater/config.toml` (else `~/.config/…`), `~/.vultr-dns-updater.toml`, `./vultr-dns-updater.toml`.

**Version scheme**: Repo always at `0.0.0`. The release workflow patches the real version from the git tag via `sed` (prerelease suffixes kept: tag `v1.0.0-rc.1` builds `1.0.0-rc.1`) and fails if `0.0.0` survives. The `binary-release` feature switches self update from `cargo install` to the GitHub Releases binary.

## systemd units

`service.rs` writes the same unit texts as my_infra's `rustible/playbooks/ourserver/files/vultr-dns-updater.{service.j2,timer}`. `tests/fixtures/` holds them as rendered on OURSERVER (user `cadu`, home `/home/cadu`, the binary in `~/.local/bin` as `install.sh` puts it), and unit tests require byte-identical output. If my_infra's files change, re-render them here; if these texts change, change my_infra's too.

## `vultr-dns usage` (agent entry point)

- Plain text in the style of `tu usage` / `pygmy usage`: a hand-written string in `usage.rs`, UPPERCASE headings, two aligned columns, no Markdown.
- `usage` ≠ `--help`. `usage` = "what can I do?" for agents; `--help` = clap detail for humans.
- Under ~1000 tokens (a test caps it at 4000 bytes). Defaults (`DEFAULT_TTL`, `DEFAULT_INTERVAL`) are rendered from the constants.
- **Update `usage.rs` whenever commands, flags or semantics change.**

## Test-only environment variables

Not in `--help` or `usage`; the tests point the binary at mock servers with them.

| Variable | Overrides |
|---|---|
| `VULTR_DNS_UPDATER_API_URL` | The Vultr API base, `https://api.vultr.com/v2` |
| `VULTR_DNS_UPDATER_GITHUB_API_URL` | The GitHub API base, `https://api.github.com` |
| `VULTR_DNS_UPDATER_RELEASES_URL` | The release download base, `https://github.com/flipbit03/vultr-dns-updater/releases/download` |

Tests never reach the internet: `tests/cli.rs` clears `VULTR_API_KEY`, isolates `HOME`/cwd in a temp dir and points all three at mocks or nowhere. Keep it that way; there are no online tests.

## CI/CD

- `ci.yml` (push to main, PRs): fmt, clippy, doc lints, tests, release builds for the three targets (Linux x86_64/aarch64 musl, macOS aarch64), a static-link check on the Linux binaries, a `--version` smoke test.
- `release.yml`: triggered by publishing a GitHub Release. Checks the tag is semver, patches the version (with the `0.0.0` guard), builds the three targets, checks the Linux binaries are static, smoke-tests `--version` against the tag, writes `<asset>.sha256`, attests build provenance (`actions/attest-build-provenance`), uploads binaries and checksums to the release, and **then, last,** publishes to crates.io with `CARGO_REGISTRY_TOKEN`: a failed build never publishes a crate, and a crates.io failure never leaves a release without binaries. **A prerelease skips crates.io**; everything else runs. No `-Dwarnings` in release builds (lints are CI's job).
- `install.sh`: curl-based installer, all inside `main()` so a truncated download runs nothing. Latest release, or `VULTR_DNS_VERSION=<tag>` (prereleases too); verifies the `.sha256` (fails without `sha256sum`/`shasum`); stages the binary in the install dir (`/tmp` may be noexec), requires `--version` to print the tag's version, then renames it into `VULTR_DNS_INSTALL_DIR` (default `~/.local/bin`).

## Releasing

Never run `cargo publish` by hand, and never set a version in `Cargo.toml`. Check the current version first (`gh release list --limit 5`), then:

```bash
# Rehearse: a prerelease runs the whole pipeline except crates.io.
gh release create v1.0.0-rc.1 --prerelease --generate-notes --target main
gh run watch    # the Release workflow
VULTR_DNS_VERSION=v1.0.0-rc.1 VULTR_DNS_INSTALL_DIR=/tmp/vdu sh install.sh && /tmp/vdu/vultr-dns --version

# Release for real.
gh release create v1.0.0 --generate-notes --target main
```

Assets are `vultr-dns_<os>_<arch>` plus `.sha256`. `releases/latest` (what `install.sh` and `self update` use) ignores prereleases.

## Git Commits

- Do NOT include the "Generated with Claude Code" footer in commits or PR descriptions
