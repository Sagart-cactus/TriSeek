FROM rust:latest

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        bash \
        ca-certificates \
        coreutils \
        git \
        jq \
        procps \
        ripgrep \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace
