# kaas-ui, in three stages.
#
# Order matters: `rust-embed` pulls `web/dist` in at *compile* time, so the
# frontend stage must precede the Rust stage. Getting it backwards produces an
# image that builds cleanly and serves 404s.

# --- 1. the frontend -------------------------------------------------------
FROM node:24-alpine AS web

WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci

COPY web/ ./
RUN npm run build


# --- 2. the backend, static musl -------------------------------------------
# No C toolchain at runtime and no librdkafka: kaas-lib is pure Rust, and
# rustls with `ring` rather than `aws-lc-sys` is what keeps this two lines
# instead of a cmake project.
#
# The image tag is the toolchain pin for this build, and it must match
# rust-toolchain.toml. `rust-toolchain.toml` is deliberately *not* copied in:
# it asks for rust-src and rust-analyzer, which a release build has no use for
# and which rustup would spend a minute downloading into a layer that is
# thrown away.
#
# Alpine images are already musl-hosted, so the target needs no `rustup target
# add`. `musl-dev` is for `ring`, which needs a C compiler and nothing else.
FROM rust:1.97.1-alpine AS build

RUN apk add --no-cache musl-dev

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY .cargo/ .cargo/
COPY crates/ crates/
COPY xtask/ xtask/
# The compiled-in frontend. Copied before `cargo build`, because it is an
# input to it.
COPY --from=web /web/dist/ web/dist/

RUN cargo build --release --locked --target x86_64-unknown-linux-musl -p kaas-ui-server \
    && cp target/x86_64-unknown-linux-musl/release/kaas-ui /kaas-ui


# --- 3. distroless ---------------------------------------------------------
FROM gcr.io/distroless/static-debian12:nonroot

COPY --from=build /kaas-ui /kaas-ui

USER 65532:65532
EXPOSE 8080

# There is no shell in this image, so this is exec form by necessity as well
# as by preference.
ENTRYPOINT ["/kaas-ui"]
CMD ["--config", "/etc/kaas-ui/config.yaml"]
