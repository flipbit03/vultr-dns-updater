# Contributing

## Setup

Install Rust from [rustup.rs](https://rustup.rs), then:

```bash
git clone git@github.com:flipbit03/vultr-dns-updater.git
cd vultr-dns-updater
cargo build
```

## Commands

```bash
make check      # fmt --check, clippy (warnings are errors), doc lints
make test       # unit + integration tests (offline: Vultr and the IP services are mocked)
make install    # cargo install --path . --locked
```

`make check && make test` is what CI runs; run both before pushing.

## Pull requests

Work on a branch and open a PR to `main`; CI must pass before merging. See [CLAUDE.md](CLAUDE.md) for the architecture, the conventions and how releases work.
