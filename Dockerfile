FROM rust:1.85.0 AS builder
WORKDIR /build
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /build/target/release/modelun /app/modelun
COPY --from=builder /build/client /app/client
RUN chmod +x /app/modelun
ENV RUST_LOG=info
EXPOSE 3000
RUN useradd -ms /bin/bash appuser
USER appuser
CMD ["./modelun"]
