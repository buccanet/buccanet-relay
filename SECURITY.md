# Security policy

## Reporting a vulnerability

Report vulnerabilities privately through a GitHub security advisory: https://github.com/buccanet/buccanet-relay/security/advisories/new

Please do not open a public issue for a vulnerability. Include the affected commit, the steps to reproduce and the impact you observed.

A vulnerability in the protocol, the MLS client code, the SDK or the CLI belongs to the core repository: https://github.com/buccanet/buccanet/security/advisories/new

## Supported versions

Security fixes land on the `main` branch, which https://app.buccanet.com runs.

## Scope

The relay holds no encryption key, and clients decide membership and authorship from the MLS group. Reports that matter most for the relay:

1. Reading or altering another device's inbox, rooms or invites.
2. Bypassing room admin checks, join approval or rate limits.
3. Obtaining a bearer token or an invite token.
4. Denial of service that a single client can trigger cheaply.
5. Weaknesses in the security headers or the Content Security Policy of the served web app.

The trust model is described in https://github.com/buccanet/buccanet/blob/main/docs/PROTOCOL.md
