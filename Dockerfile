# Self hosting image of the relay. The web app's image (buccanet-web) builds on this one and
# serves the app from the same origin through STATIC_DIR.
FROM rust:1.94-trixie AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY vendor/buccanet ./vendor/buccanet
RUN cargo build --release --locked && cp target/release/buccanet-relay /usr/local/bin/buccanet-relay

FROM debian:trixie-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /usr/local/bin/buccanet-relay /usr/local/bin/buccanet-relay
ENV BIND_HOST=0.0.0.0 PORT=8758 MESSAGE_TTL_DAYS=30
EXPOSE 8758
CMD ["buccanet-relay"]
