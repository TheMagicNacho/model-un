# Stage 1: Builder stage
FROM rust:bullseye AS builder
#RUN apk add --no-cache libgcc
WORKDIR /app
COPY src/ /app/src/
COPY Cargo.toml /app
RUN cargo build --release
RUN ldd ./target/release/modelun


# Stage 2: Final stage
FROM debian:latest
WORKDIR /app
COPY --from=builder /app/target/release/modelun /app/modelun
COPY client/ /app/client
RUN chmod +x /app/modelun
ENV RUST_LOG info
EXPOSE 3000
CMD ["./modelun"]