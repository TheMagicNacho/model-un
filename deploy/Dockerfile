FROM debian:latest
WORKDIR /app
COPY ../target/release/modelun /app/modelun
COPY ../client /app/client
RUN chmod +x /app/modelun
ENV RUST_LOG info
EXPOSE 3000
CMD ["./modelun"]