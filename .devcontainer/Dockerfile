FROM dr.rechenknecht.net/bauhelfer/container/main/rust:ubuntu-resolute

ARG REMOTE_USER=developer
ARG REMOTE_UID=992
ARG REMOTE_GID=992

RUN rustup component add rust-src
RUN rustup component add rustfmt
RUN rustup component add clippy
RUN cargo install cargo-audit

RUN groupadd --gid $REMOTE_GID $REMOTE_USER \
    && useradd --uid $REMOTE_UID --gid $REMOTE_GID -m $REMOTE_USER -s /bin/bash -d /home/developer

# Allow calling sudo in devcontainer (e.g. to install additional packages), enable if required
# RUN apt-get update && apt-get install -y sudo
# RUN echo $REMOTE_USER ALL=\(root\) NOPASSWD:ALL > /etc/sudoers.d/$REMOTE_USER

RUN mkdir /cargo_home
# We do not know the user ID right now, but need the home directory for cargo later
RUN chmod 777 /cargo_home
ENV CARGO_HOME=/cargo_home

ENV RUSTFLAGS="-Dunused-crate-dependencies"

# This must be the last step as the REMOTE_UID is not set here, apparently it is not possible
# to setup users properly during container build.
USER $REMOTE_USER
