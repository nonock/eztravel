FROM rust:1.99-slim-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked

FROM debian:bookworm-slim
# HTTPS to PRIM, Google and Firebase is checked against the system's certificates.
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/eztravel-backend /usr/local/bin/eztravel-backend
CMD ["eztravel-backend"]
