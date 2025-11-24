FROM rust:1.91.1-slim AS chef

RUN apt-get update && \
    apt-get install -y musl-tools pkg-config build-essential libssl-dev && \
    rustup target add x86_64-unknown-linux-musl && \
    rm -rf /var/lib/apt/lists/*

RUN cargo install cargo-chef && \
    cargo install sccache

ENV RUSTC_WRAPPER=sccache \
    SCCACHE_DIR=/sccache

WORKDIR /enginewhy

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /enginewhy/recipe.json recipe.json

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=$SCCACHE_DIR,sharing=locked \
    cargo chef cook --release --target x86_64-unknown-linux-musl --recipe-path recipe.json

COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=$SCCACHE_DIR,sharing=locked \
    cargo build --release --target x86_64-unknown-linux-musl

# change to scratch and get comment the apk command for prod, i guess
FROM alpine:latest AS runtime
# RUN apk add --no-cache ca-certificates curl netcat-openbsd bind-tools strace
WORKDIR /enginewhy
COPY --from=builder /enginewhy/target/x86_64-unknown-linux-musl/release/l4lb /usr/bin/l4lb
ENTRYPOINT ["l4lb"]