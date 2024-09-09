FROM almalinux:8 AS builder_base

RUN yum -y update && \
    yum -y groupinstall "Development Tools" && \
    yum -y install openssl-devel && \
    yum -y install glibc-devel

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y 

ENV PATH="/root/.cargo/bin:${PATH}"
RUN cargo install cargo-c

FROM builder_base AS builder

WORKDIR /build
COPY . .
RUN cargo cbuild -p yara-x-capi --release --target x86_64-unknown-linux-gnu --target-dir /build/artifacts

RUN mkdir -p /out
RUN cp -r /build/artifacts/x86_64-unknown-linux-gnu/release/libyara_x_capi.so /out/
