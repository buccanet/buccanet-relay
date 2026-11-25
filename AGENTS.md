# Instructions for coding agents

This file tells coding agents how to work in `buccanet-relay`. `CLAUDE.md` holds the same project rules for Claude Code.

## What this repository is

The Buccanet relay, an Axum server in Rust that orders, stores and delivers sealed MLS messages between Buccanet devices. It holds no encryption key. MongoDB stores its data. The HTTP API it serves is specified in `vendor/buccanet/docs/PROTOCOL.md`.

## Layout

1. `src/main.rs`: startup, the `health` subcommand, graceful shutdown on SIGTERM.
2. `src/config.rs`: every environment variable the relay reads.
3. `src/routes/`: transport only (routing, extraction, CORS, security headers).
4. `src/features/<feature>/`: business rules. Persistence lives in `repo.rs` files. The features are `devices`, `rooms` and `delivery`.
5. `src/auth.rs`, `src/client_addr.rs`, `src/limits.rs`, `src/locks.rs`, `src/tokens.rs`: bearer authentication, client address, rate limits, per key locks, token hashing.
6. `vendor/buccanet`: the core as a git submodule, pinned to a core commit. The relay uses its `buccanet-protocol` crate for the protocol types. Do not edit it from this repository.

## Commands

```sh
git submodule update --init      # fetch the core into vendor/buccanet
cp .env.example .env
set -a; . ./.env; set +a
cargo run                        # needs MongoDB on localhost, listens on 8758
cargo fmt --check                # formatting, configured in rustfmt.toml
cargo clippy --all-targets       # lint
cargo test                       # unit tests
docker build -t buccanet-relay:dev .
```

Run `cargo fmt --check`, `cargo clippy --all-targets` and `cargo test` before proposing a change.

## Rules

1. No query strings anywhere: links, redirects and API inputs carry their parameters in the path or in a JSON body. Tokens appear in a URL only in their dedicated path, such as invite tokens in `/join/<token>`.
2. Routes handle transport only. Business rules go in `src/features/<feature>/`, and persistence in its `repo.rs`.
3. Functions stay around 40 lines and files around 400 lines.
4. The relay reads no message content. A change that needs plaintext belongs in the core, on the client side.
5. A change to the API changes `vendor/buccanet/docs/PROTOCOL.md` and the `buccanet-protocol` types in the core repository first.
6. Scratch work goes in `.tmp/`, which git ignores.
7. Copy, docs, comments and commit messages use plain declarative sentences and no dash as punctuation.

## License

AGPL 3.0, also sold under a commercial license (`COMMERCIAL.md`). Contributions are licensed under AGPL 3.0.
