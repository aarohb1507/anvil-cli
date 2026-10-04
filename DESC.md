# Anvil

Anvil is now a small durable step runner.

It is intentionally not the whole workflow platform. If the main orchestration
project is moving to Go, this Rust crate can still be useful as a tiny CLI that
wraps expensive or flaky steps and remembers successful output.

## What It Does

Run a step once:

```sh
anvil run summarize:page-80 page-80.txt -- sh -c 'echo summary text'
```

Anvil writes an append-only diary to `.anvil/diary.tsv`:

1. `started` before running the command
2. `complete` with captured stdout if the command succeeds
3. `failed` with the exit code if the command fails

If the same step is run again with the same input text, Anvil prints the cached
stdout and skips the command.

## Why This Makes Sense Beside Go

Let Go own the API, queues, retries, workers, and product logic.

Let this crate be the simple durable boundary:

```sh
anvil run embed:doc-42 doc-42-v1 -- ./embed-doc doc-42.pdf
```

That gives any caller a concrete guarantee:

> If this exact step already completed for this exact input, reuse the old
> output instead of doing the work again.

## Commands

```sh
anvil run [--diary PATH] <step-id> <input> -- <command> [args...]
anvil status [PATH]
```

## Testing

The end-to-end test launches the built `anvil` binary twice against a temporary
diary. It verifies that the first call runs the wrapped shell command, the
second returns cached stdout, and `status` reports the completed step.

```sh
cargo test --test durable_runner_e2e -- --nocapture
```

With `--nocapture`, the test prints two lightweight timing metrics:
`fresh_run_ms` and `cache_hit_ms`. They include CLI process startup and diary
I/O, making them useful as a small regression signal rather than a synthetic
microbenchmark.

## Current Storage Format

The diary is a tab-separated append-only file. That keeps the first useful
version easy to inspect, easy to delete during development, and easy for the Go
project to read while the product shape is still changing.

Later, the same contract can move to SQLite, Postgres, S3, or a Go-native
storage layer without changing the core idea.
