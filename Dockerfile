FROM debian:stable
ARG BINARY

RUN apt update && apt install -y libssl3 ca-certificates && rm -rf /var/lib/apt/lists/*

RUN mkdir -p /kids
WORKDIR "/kids"

COPY target/release/$BINARY /usr/local/bin/kids
ENTRYPOINT ["/usr/local/bin/kids"]
