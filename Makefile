.PHONY: lint typecheck check install dev test

install:
	uv sync

dev:
	uv sync --group dev
	uv run pre-commit install

lint:
	uv run pre-commit run --all-files

typecheck:
	uv run mypy .

check: lint typecheck

test:
	uv run vultr-dns --help
