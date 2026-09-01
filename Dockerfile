FROM rust:latest AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock* ./
COPY src ./src
COPY dashboard.html ./
RUN cargo build --release

FROM debian:bookworm-slim
# ca-certificates required: tokio-tungstenite uses rustls-tls-native-roots
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/vsa-autonomous /usr/local/bin/
EXPOSE 10000
CMD ["vsa-autonomous"]
