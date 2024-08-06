FROM rust:1.79.0 AS yara_builder_base

RUN cargo install cargo-c
RUN apt-get update && \
    apt-get install -y libc6-dev

FROM yara_builder_base AS cloned_repo_base

ARG TAG
WORKDIR /build
RUN git clone https://github.com/catalogicsoftware/yara-x
WORKDIR /build/yara-x
RUN git fetch --all --tags
RUN git checkout $TAG

FROM cloned_repo_base AS yara_builder

WORKDIR /build/yara-x
RUN cargo cbuild -p yara-x-capi --release --target x86_64-unknown-linux-gnu --target-dir /build/artifacts

RUN mkdir -p /out
RUN cp -r /build/artifacts/x86_64-unknown-linux-gnu/release/libyara_x_capi.so /out/
