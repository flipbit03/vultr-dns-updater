# Contributing

## Setup

```bash
git clone https://github.com/flipbit03/vultr-dns-updater.git
cd vultr-dns-updater
uv sync --group dev
uv run pre-commit install
```

## Commands

```bash
make lint       # Run linting
make typecheck  # Run type checking
make check      # Run all checks
```
