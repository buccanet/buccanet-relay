# Contributing to the Buccanet relay

Thank you for helping. Bug reports, fixes and documentation improvements are welcome.

## Before you start

1. Open an issue before a large change, so the design can be discussed first.
2. A change to the HTTP API starts in the core repository (https://github.com/buccanet/buccanet), which holds `docs/PROTOCOL.md` and the `buccanet-protocol` types.
3. Report security issues privately, as described in [SECURITY.md](SECURITY.md).

## Build, test and lint

The relay needs Rust 1.91 or later, MongoDB to run, and the core submodule.

```sh
git submodule update --init
cp .env.example .env
set -a; . ./.env; set +a
cargo run
cargo fmt --check
cargo clippy --all-targets
cargo test
```

A pull request passes `cargo fmt --check`, `cargo clippy --all-targets` and `cargo test`.

## Code rules

1. Routes in `src/routes/` handle transport only. Business rules live in `src/features/<feature>/`, with persistence in `repo.rs`.
2. No query strings: parameters travel in the path or in a JSON body.
3. Functions stay around 40 lines and files around 400 lines.
4. Docs and comments use plain declarative sentences and no dash as punctuation.

## License of contributions

The relay is licensed under the GNU Affero General Public License 3.0 ([LICENSE](LICENSE)). By opening a pull request, you agree that your contribution is licensed under AGPL 3.0.

The copyright holder, Ilan Ohayon, also offers the relay under a commercial license, described in [COMMERCIAL.md](COMMERCIAL.md). Write to the address in that file with any question about licensing.

Because the relay is also sold under that commercial license, a code contribution can only be merged once its author signs a contributor license agreement, which lets the copyright holder offer the contributed code under both licenses. The maintainers send the agreement on the pull request. Issues, bug reports and design discussions need no signature.

## Code of conduct

Everyone taking part in this project follows the [code of conduct](CODE_OF_CONDUCT.md).
