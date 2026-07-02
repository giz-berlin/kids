FROM debian:stable
ARG BINARY

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

WORKDIR "/kids"

COPY target/release/$BINARY /usr/local/bin/kids
ENTRYPOINT ["/usr/local/bin/kids"]
