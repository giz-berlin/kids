FROM debian:stable AS bw
ARG BW_VERSION=2026.9.0
ARG BW_SHA256=b34fcaa0a8c96df0783e09ed6c9c22003497e5b429351c42127206b794c6b69b

RUN apt-get update && apt-get install -y --no-install-recommends curl unzip ca-certificates && \
    curl -fLo /tmp/bw.zip "https://github.com/bitwarden/clients/releases/download/cli-v${BW_VERSION}/bw-oss-linux-${BW_VERSION}.zip" && \
    echo "${BW_SHA256}  /tmp/bw.zip" | sha256sum -c - && \
    unzip /tmp/bw.zip -d /opt/bw && \
    chmod +x /opt/bw/bw

FROM debian:stable
ARG BINARY=keycloak-vaultwarden

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libatomic1 && rm -rf /var/lib/apt/lists/*
COPY --from=bw /opt/bw/bw /usr/local/bin/bw

WORKDIR "/kids"

COPY target/release/$BINARY /usr/local/bin/kids
ENTRYPOINT ["/usr/local/bin/kids"]
