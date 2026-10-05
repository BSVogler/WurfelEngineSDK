# Wurfel/Caveland server image: the game server plus the browser client it serves.
#   docker build -t wurfel-server .
#   docker run -p 3000:3000 -v wurfel-data:/data wurfel-server
FROM rust:1-bookworm AS build
ARG TRUNK_VERSION=0.21.14
RUN rustup target add wasm32-unknown-unknown \
 && curl -fsSL "https://github.com/trunk-rs/trunk/releases/download/v${TRUNK_VERSION}/trunk-$(uname -m)-unknown-linux-gnu.tar.gz" | tar -xz -C /usr/local/bin
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY wurfel-sim wurfel-sim
COPY caveland-sim caveland-sim
COPY wurfel-web wurfel-web
COPY wurfel-server wurfel-server
RUN cargo build --release -p wurfel-server
RUN cd wurfel-web && trunk build --release

FROM debian:bookworm-slim
RUN useradd --uid 10001 --create-home --shell /usr/sbin/nologin wurfel \
 && mkdir -p /data/maps && chown -R wurfel /data
COPY --from=build /src/target/release/wurfel-server /usr/local/bin/wurfel-server
COPY --from=build /src/wurfel-web/dist /app/dist
USER 10001
ENV HOME=/home/wurfel
EXPOSE 3000
VOLUME /data
ENTRYPOINT ["wurfel-server", "--port", "3000", "--static", "/app/dist", "--maps-dir", "/data/maps"]
