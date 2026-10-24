# syntax=docker/dockerfile:1
FROM debian:bookworm-slim@sha256:7c7b2c966bc9ee8cedfeef67e0e279108992c77681fa595db4a9d65c06ccc587 AS base
COPY LICENSE /usr/share/licenses/opentrack/LICENSE

FROM base AS bootstrap
RUN apt-get update && apt-get install -y --no-install-recommends python3 \
    && rm -rf /var/lib/apt/lists/*
ENV PYTHONDONTWRITEBYTECODE=1 PYTHONUNBUFFERED=1
COPY deploy/container-credentials.py /opt/opentrack/container-credentials.py
COPY deploy/test-container-credentials.py /opt/opentrack/test-container-credentials.py
ENTRYPOINT ["python3", "/opt/opentrack/container-credentials.py"]
CMD ["initialize"]

FROM node:22-bookworm-slim@sha256:c3de60bf2f9dd0ac6370e6117950ff62d6e339527e7472301c9c78a017978392 AS web
WORKDIR /build/web
COPY web/package.json web/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY web/ ./
RUN npm run build

FROM rust:1.98.1-bookworm@sha256:93ce27a88655056a51dbdd8f5f2d7ddc071c7b0070fb288a37b5a285fc83971e AS server
ARG TARGETARCH
ENV CARGO_INCREMENTAL=0
WORKDIR /build
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/ ./crates/
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=opentrack-target-${TARGETARCH},target=/build/target,sharing=locked \
    cargo build --locked --release --jobs 2 -p opentrack-server \
    && mkdir -p /build/out && cp /build/target/release/opentrack-server /build/out/opentrack-server

FROM caddy:2-alpine@sha256:d8542f48d34a9cf4e4c11a478865229840e87e4c96ea3f439101f31a5d35f75f AS proxy
COPY LICENSE /usr/share/licenses/opentrack/LICENSE
RUN mkdir -p /data/caddy /config/caddy && chown -R 10001:10001 /data /config \
    && chmod 0700 /data /data/caddy /config /config/caddy \
    && setcap -r /usr/bin/caddy
COPY deploy/Caddyfile.container /etc/caddy/Caddyfile
USER 10001:10001

FROM base AS runtime
ARG OPENTRACK_SERVICE_UID=10001
ARG OPENTRACK_SERVICE_GID=10001
# The runtime uses rustls; trust roots and a healthcheck client are still needed.
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && test "$OPENTRACK_SERVICE_UID" -gt 0 && test "$OPENTRACK_SERVICE_GID" -gt 0 \
    && groupadd --gid "$OPENTRACK_SERVICE_GID" opentrack \
    && useradd --uid "$OPENTRACK_SERVICE_UID" --gid opentrack --no-create-home --shell /usr/sbin/nologin opentrack \
    && install -d -o opentrack -g opentrack /var/lib/opentrack /app/web \
    && install -d -m 0700 -o opentrack -g opentrack /var/lib/opentrack/blobs
WORKDIR /app
COPY --from=server /build/out/opentrack-server /usr/local/bin/opentrack-server
COPY --from=web /build/web/dist/ /app/web/dist/
USER ${OPENTRACK_SERVICE_UID}:${OPENTRACK_SERVICE_GID}
EXPOSE 8080
ENTRYPOINT ["opentrack-server"]
CMD ["serve"]
