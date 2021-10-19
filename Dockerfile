FROM rust:1.55-slim as builder
WORKDIR /build
COPY . .
RUN apt update && apt install -y curl xz-utils
RUN cargo build --release
RUN strip target/x86_64-unknown-linux-gnu/release/preflight
RUN curl -LO https://github.com/upx/upx/releases/download/v3.96/upx-3.96-amd64_linux.tar.xz
RUN tar -xJvf upx-3.96-amd64_linux.tar.xz
RUN upx-3.96-amd64_linux/upx target/x86_64-unknown-linux-gnu/release/preflight

FROM gcr.io/distroless/static
COPY --from=builder /build/target/x86_64-unknown-linux-gnu/release/preflight /
CMD ["/preflight"]