# Project rules

## What this is

Buccanet: end to end encrypted group chats where AI agents and people talk together, on every platform. One Rust core (MLS through OpenMLS), in the `buccanet` repository, is shared by the CLI, the web app (WebAssembly) and the native apps (UniFFI). The relay only orders and stores sealed messages. Local dev: relay on 8758, web app on 8759. The protocol, SDK and roadmap live in `buccanet/docs`.

## Repositories

`buccanet` is the open core. `buccanet-relay`, `buccanet-web`, `buccanet-linux`, `buccanet-apple`, `buccanet-android` and `buccanet-windows` each build on it through the `vendor/buccanet` git submodule, pinned to a core commit. Locally the seven repositories sit side by side in one folder.

## No query strings, anywhere

Query strings (`?key=value`) are forbidden across every repository: apps, relay, redirects and internal links. State travels in the path, in the request body, in headers or in component state. External URLs we call are exempt.

- Links and redirects carry their parameters as path segments.
- API inputs are path parameters or JSON bodies.
- Tokens never appear in a URL, except opaque tokens in their dedicated path: invite tokens in `/join/<token>` are allowed because they are reusable by design, expire after 7 days, can be revoked, and every join they start still waits for an admin.

## Layout

- `src/routes/`: transport only.
- `src/features/<feature>/`: business rules, persistence in `repo.rs` files.
- `vendor/buccanet`: the core, used for the protocol types.

Functions stay around 40 lines, files around 400. Scratch work goes in `.tmp/`.

## Licensing

AGPL 3.0, also sold under a commercial license (`COMMERCIAL.md`).

## Writing Rules

All copy and writing must follow the writing rules in `/Users/ilan/Projects/utils/skillslib/plugins/ilan-skills/skills/_shared/references/writing-quality.md`.
