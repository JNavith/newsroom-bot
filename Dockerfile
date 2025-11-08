FROM rust:1.89-alpine3.22 AS build

RUN apk add --no-cache musl-dev

WORKDIR /app
COPY . .

RUN cargo build --release --package on-tunnel-service-or-exposed-port && \
    cp ./target/release/on-tunnel-service-or-exposed-port /executable && \
    rm -rf ./target


FROM scratch AS runner

COPY --from=build /executable /executable

ENTRYPOINT ["/executable"]
