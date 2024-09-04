FROM rust:1.79.0 AS yara_builder_base

RUN cargo install cargo-c
RUN rustup target add x86_64-unknown-linux-musl
RUN apt-get update && \
    apt-get install -y libc6-dev musl-tools

FROM yara_builder_base AS yara_builder

WORKDIR /build
COPY . .
RUN cargo cbuild -p yara-x-capi --release --target x86_64-unknown-linux-musl --target-dir /build/artifacts

RUN mkdir -p /convert
RUN cp /build/artifacts/x86_64-unknown-linux-musl/release/libyara_x_capi.a /convert/

WORKDIR /convert
RUN musl-gcc -shared -o libyara_x_capi.so -static-libgcc -static-libstdc++ -Wl,--whole-archive libyara_x_capi.a -Wl,--no-whole-archive

RUN mkdir -p /out
RUN cp /convert/libyara_x_capi.so /out/
