FROM rust:1.79.0 AS yara_builder_base

RUN cargo install cargo-c
RUN apt-get update && \
    apt-get install -y libc6-dev

FROM yara_builder_base AS yara_builder

WORKDIR /build
COPY . .
RUN cargo cbuild -p yara-x-capi --release --target x86_64-unknown-linux-gnu --target-dir /build/artifacts

RUN mkdir -p /out
RUN cp -r /build/artifacts/x86_64-unknown-linux-gnu/release/libyara_x_capi.so /out/
