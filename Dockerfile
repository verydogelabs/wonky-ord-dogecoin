FROM rust:slim-bullseye as ord-builder

RUN apt-get update && \
    apt-get install -y \
    ca-certificates curl file git build-essential libssl-dev pkg-config

RUN mkdir /app

COPY ./*.json /app
COPY ./*.toml /app
COPY ./*.sh /app
COPY ./LICENSE /app
COPY ./*.svg /app
COPY ./Cargo.lock /app

RUN mkdir -p /app/templates
COPY ./templates /app/templates

RUN mkdir -p /app/static
COPY ./static /app/static

RUN mkdir -p /app/src
COPY ./src /app/src

RUN mkdir -p /app/fuzz
COPY ./fuzz /app/fuzz

RUN mkdir -p /app/test-bitcoincore-rpc
COPY ./test-bitcoincore-rpc /app/test-bitcoincore-rpc

COPY ./justfile /app

WORKDIR /app

RUN cp starting_sats.json /starting_sats.json
RUN cp subsidies.json /subsidies.json
RUN cargo build --release

# ----------------------------------------------------------------------------------------------------------------------
FROM rust:slim-bullseye

RUN apt-get update && \
    apt-get install -y \
    ca-certificates curl file git build-essential libssl-dev pkg-config

COPY --from=ord-builder /app/target/release/ord /usr/local/bin/ord
COPY --from=ord-builder /subsidies.json /subsidies.json
COPY --from=ord-builder /starting_sats.json /starting_sats.json

RUN mkdir /root/.data
