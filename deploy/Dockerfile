# Multi-stage production Dockerfile for `automatafl.fg-goose.online`
# Builds `automatafl-server` and `automatafl-client` with LTO + strip into a minimal runtime container.

FROM rust:1.85-bookworm AS builder
WORKDIR /src

# If Lean-emitted C files in `lean/.lake/build/ir/Automatafl/` are not pre-populated in the build context,
# install `elan` and run `lake build` automatically before invoking `cargo build --release`.
COPY Cargo.toml Cargo.lock ./
COPY v8n ./v8n
COPY automatafl-lean ./automatafl-lean
COPY automatafl-ai ./automatafl-ai
COPY automatafl-server ./automatafl-server
COPY automatafl-client ./automatafl-client
COPY lean ./lean

RUN if [ ! -f lean/.lake/build/ir/Automatafl/FFI.c ]; then \
      curl -sSf https://raw.githubusercontent.com/leanprover/elan/master/elan-init.sh | sh -s -- -y --default-toolchain none && \
      export PATH="/root/.elan/bin:${PATH}" && \
      cd lean && lake build; \
    fi && \
    export PATH="/root/.elan/bin:${PATH}" && \
    cargo build --release -p automatafl-server -p automatafl-client

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && rm -rf /var/lib/apt/lists/*
WORKDIR /app

COPY --from=builder /src/target/release/automatafl-server /usr/local/bin/automatafl-server
COPY --from=builder /src/target/release/automatafl-client /usr/local/bin/automatafl-client

RUN mkdir -p /data
VOLUME ["/data"]

ENV AUTOMATAFL_BIND=0.0.0.0:3030
ENV AUTOMATAFL_DATA_DIR=/data
EXPOSE 3030

HEALTHCHECK --interval=15s --timeout=3s --start-period=5s --retries=3 \
  CMD curl -fsS http://127.0.0.1:3030/api/health || exit 1

ENTRYPOINT ["/usr/local/bin/automatafl-server"]
CMD ["--bind", "0.0.0.0:3030", "--data-dir", "/data"]
