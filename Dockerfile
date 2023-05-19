FROM debian:latest

ARG DOGECOIN_RPC_PORT

WORKDIR /app

# common packages
RUN apt-get update && \
    apt-get install -y \
    ca-certificates curl file git build-essential libssl-dev

# install toolchain
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs --output rustup-init.sh
RUN sh ./rustup-init.sh -y
ENV PATH=/root/.cargo/bin:$PATH
RUN git clone https://github.com/verydogelabs/wonky-ord-dogecoin.git  \
    && cd wonky-ord-dogecoin  \
    && cargo build --release  \
    && cp ./target/release/ord /usr/local/bin/ord

RUN export RUST_LOG=debug
