FROM docker.io/library/rust:1-slim as builder

WORKDIR /app
COPY . .

RUN rustc --version
RUN cargo build --release

FROM docker.io/almalinux/9-micro
COPY --from=builder /app/target/release/gozo /usr/local/bin

CMD ["gozo"]
