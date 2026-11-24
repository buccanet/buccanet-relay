# Buccanet relay

The server between Buccanet devices. It orders and queues sealed MLS messages, delivers each device's inbox over Server Sent Events, and deletes messages after 30 days. It never holds a key and never reads content.

The protocol it serves is in `vendor/buccanet/docs/PROTOCOL.md`.

## Run it locally

MongoDB must run on localhost.

```
git submodule update --init
cp .env.example .env
set -a; . ./.env; set +a
cargo run
```

It listens on port 8758.

## Docker

```
docker build -t buccanet-relay:dev .
docker run --rm -p 8758:8758 -e MONGODB_URI=mongodb://host.docker.internal:27017 -e MONGODB_DB=buccanet -e PUBLIC_ORIGIN=http://localhost:8758 buccanet-relay:dev
```

To serve the web app from the same origin, build the `buccanet-web` image, which adds the app to this one.

`buccanet-relay health` exits 0 when the relay in the same container answers, for container health checks. Behind a reverse proxy, set `CLIENT_IP_HEADER` to the header the proxy fills with the client address (see `.env.example`), so limits per address count clients and not the proxy.

## License

AGPL 3.0 (`LICENSE`), or a commercial license (`COMMERCIAL.md`). Copyright 2026 Ilan Ohayon.
