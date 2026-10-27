.PHONY: check rtl-test
check:
	uv run --locked ruff check .
	uv run --locked ruff format --check .
	uv run --locked mypy
	uv run --locked pytest

rtl-test:
	uv run --locked python tests/rtl/run.py "$(HW_ROOT)"
