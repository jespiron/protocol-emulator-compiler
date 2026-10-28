.PHONY: check rtl-test docs
check:
	uv run --locked --project tests/reference/python ruff check assets/simulator tests/rtl tests/differential.py tests/test_runner.py
	uv run --locked --project tests/reference/python ruff format --check assets/simulator tests/rtl tests/differential.py tests/test_runner.py
	cargo fmt --check
	cargo clippy --locked --all-targets -- -D warnings
	cargo test --locked
	RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps
	cargo build --locked
	uv run --locked --project tests/reference/python pytest tests/reference/python/tests
	uv run --locked --project tests/reference/python python tests/differential.py
	uv run --locked --project tests/reference/python python tests/test_runner.py

rtl-test:
	cargo build --locked
	uv run --locked --project tests/reference/python python tests/rtl/check.py
	uv run --locked --project tests/reference/python python tests/rtl/workflow.py target/debug/spatial

docs:
	cargo doc --locked --no-deps --open
