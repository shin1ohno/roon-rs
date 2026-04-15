FROM rust:1-bookworm AS builder
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
RUN cargo build --release -p roon-hub

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /src/target/release/roon-hub /usr/local/bin/roon-hub
COPY roon-hub.toml.example /etc/roon-hub/roon-hub.toml
WORKDIR /etc/roon-hub
EXPOSE 1883
ENTRYPOINT ["roon-hub"]
CMD ["/etc/roon-hub/roon-hub.toml"]
