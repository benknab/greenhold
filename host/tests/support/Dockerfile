FROM debian:bookworm-slim@sha256:7c7b2c966bc9ee8cedfeef67e0e279108992c77681fa595db4a9d65c06ccc587
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl socat \
    && rm -rf /var/lib/apt/lists/*
# Utreexod v0.6.0 release archives. No local binaries or chain snapshots.
RUN set -eu; arch="$(dpkg --print-architecture)"; \
    case "$arch" in \
      amd64) sha=815a66bf8ff53aaa9cdd1d6ed39bf6a305ff644a784f56fe88dc3d599e292383 ;; \
      arm64) sha=193861b6ed55e91dd51745ad72f041a804b00d74b35d499004f16fd7c7980fe7 ;; \
      *) exit 1 ;; \
    esac; \
    curl --fail --location --retry 3 -o /node.tar.gz \
      "https://github.com/utreexo/utreexod/releases/download/v0.6.0/utreexod-linux-${arch}-20260623-01.tar.gz"; \
    echo "$sha  /node.tar.gz" | sha256sum -c -; \
    mkdir /unpack; tar -xzf /node.tar.gz -C /unpack; \
    find /unpack -type f -name utreexod -exec install -m 755 '{}' /usr/local/bin/utreexod \; ; \
    test -x /usr/local/bin/utreexod; rm -rf /unpack /node.tar.gz
COPY entrypoint.sh /entrypoint.sh
ENTRYPOINT ["sh", "/entrypoint.sh"]
