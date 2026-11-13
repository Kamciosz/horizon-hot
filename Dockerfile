# syntax=docker/dockerfile:1
# PL: Obraz serwera horizon-hot (M8.1c, ADR 0011). Tylko programy: hotd, hot-proxy, hot-core,
#     pula slotów oraz horizon-server i horizon-engine zbudowane ze źródeł Horizona w przypiętej
#     rewizji (jak packaging/server/Dockerfile Horizona; datowane wydania Nightly znikają po kilku
#     dniach). Pliki gry, świat, klucze i logi są w wolumenach, a nie w obrazie. Tylko linux/amd64.
# EN: The horizon-hot server image (M8.1c, ADR 0011). Programs only: hotd, hot-proxy, hot-core,
#     the slot pool, and horizon-server and horizon-engine built from Horizon's source at the pinned
#     revision (as Horizon's packaging/server/Dockerfile does; dated Nightly releases disappear after
#     a few days). Game files, the world, keys and logs live in volumes, never in the image. linux/amd64 only.

ARG HORIZON_VERSION=nightly-2026-11-05-66f3981
ARG HORIZON_REV=66f3981f522b24c250b21523a70080965b940424

# --- PL: Budowa. EN: Build. ------------------------------------------------------------------
FROM ubuntu:24.04 AS rust
RUN apt-get update -q \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y -q --no-install-recommends \
      build-essential ca-certificates curl git pkg-config \
 && rm -rf /var/lib/apt/lists/*
ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo PATH=/opt/cargo/bin:$PATH
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable --no-modify-path

# PL: Horizon ze źródeł, w rewizji sprawdzonej przez git (pełny hash). Kompilacja nie pobiera nic
#     od Mojanga: pliki gry montuje właściciel przy starcie.
# EN: Horizon from source, at a revision git checks (full hash). The build downloads nothing from
#     Mojang: the owner mounts the game files at start.
FROM rust AS horizon
ARG HORIZON_REV
WORKDIR /horizon
RUN git init -q . && git remote add origin https://github.com/horizon-rust/horizon \
 && git fetch -q --depth 1 origin "$HORIZON_REV" && git checkout -q FETCH_HEAD \
 && test "$(git rev-parse HEAD)" = "$HORIZON_REV"
RUN --mount=type=cache,target=/opt/cargo/registry --mount=type=cache,target=/opt/cargo/git \
    --mount=type=cache,target=/horizon/target \
    cargo build --release -q --locked -p horizon-client --no-default-features --bin horizon-client \
 && cargo build --release -q --locked -p horizon-server --bin horizon-server \
 && cargo build --release -q --locked -p horizon-bots --bin horizon-bots \
 && mkdir -p /out \
 && cp target/release/horizon-client /out/horizon-engine \
 && cp target/release/horizon-server target/release/horizon-bots /out/

FROM rust AS build
ARG HORIZON_REV
WORKDIR /src
COPY . .
# PL: Te same paczki co w instalatorze (krok 5). EN: The same packages as the installer (step 5).
RUN --mount=type=cache,target=/opt/cargo/registry --mount=type=cache,target=/opt/cargo/git \
    --mount=type=cache,target=/src/target \
    cargo build --release -q --locked -p hotd -p hot-core -p hot-diag -p hot-proxy \
 && mkdir -p /out/bin /out/lib \
 && cp target/release/hotd target/release/hot-diag target/release/hot-proxy /out/bin/ \
 && cp target/release/libhot_core.so /out/lib/
# PL: Pula slotów z tej samej wersji Horizona co serwer (krok 9 instalatora); build jest powtarzalny.
# EN: The slot pool from the server's Horizon version (installer step 9); the build is reproducible.
RUN --mount=type=cache,target=/opt/cargo/registry --mount=type=cache,target=/opt/cargo/git \
    cargo install -q --locked --git https://github.com/horizon-rust/horizon --rev "$HORIZON_REV" cargo-horizon-mod \
 && /out/bin/hotd slots-source /tmp/hot-slots \
 && cd /tmp/hot-slots && cargo horizon-mod build \
 && cp target/horizon-mod/hot_slots.hmod /out/
ARG HOT_SHA=main
# PL: hot.yml dla repo serwera, przypięty do wersji horizon-hot z obrazu.
# EN: hot.yml for the server repo, pinned to the image's horizon-hot version.
RUN sed "s|^\(          ref: \)main$|\1$HOT_SHA|" templates/server-repo/.github/workflows/hot.yml > /out/hot.yml \
 && sh crates/hot-host/build-bundle.sh /out/hot-host-bundle

# --- PL: Obraz do uruchomienia. EN: Runtime image. -------------------------------------------
FROM ubuntu:24.04
ARG HORIZON_VERSION
ARG HORIZON_REV
ARG HOT_SHA=main
RUN apt-get update -q \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y -q --no-install-recommends \
      ca-certificates curl git openssh-client tar \
 && rm -rf /var/lib/apt/lists/*
COPY --from=horizon /out/ /opt/horizon/
RUN useradd --system --home-dir /srv/horizon --shell /usr/sbin/nologin horizon \
 && mkdir -p /srv/horizon && chown horizon:horizon /srv/horizon
COPY --from=build /out/bin/ /opt/horizon-hot/bin/
COPY --from=build /out/lib/ /opt/horizon-hot/lib/
COPY --from=build /out/hot_slots.hmod /out/hot.yml /opt/horizon-hot/
COPY --from=build /out/hot-host-bundle/ /opt/horizon-hot/hot-host-bundle/
COPY scripts/container-entrypoint.sh /opt/horizon-hot/entrypoint.sh
ENV HORIZON_VERSION=$HORIZON_VERSION HORIZON_REV=$HORIZON_REV HOT_SHA=$HOT_SHA HORIZON_NO_AUDIO=1
VOLUME /srv/horizon
EXPOSE 25565 8131
# PL: Status przez proxy, jak lista serwerów w grze: zdrowy = gracz dojdzie do silnika.
# EN: The status through the proxy, as the game's server list asks: healthy = a player reaches the engine.
HEALTHCHECK --interval=30s --timeout=15s --start-period=180s --retries=3 \
  CMD /opt/horizon-hot/bin/hot-proxy ping 127.0.0.1:25565 > /dev/null || exit 1
STOPSIGNAL SIGTERM
ENTRYPOINT ["/opt/horizon-hot/entrypoint.sh"]
