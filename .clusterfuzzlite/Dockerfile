FROM gcr.io/oss-fuzz-base/base-builder-rust
# The distro's protobuf-compiler (3.6.x on Ubuntu 20.04) predates
# --experimental_allow_proto3_optional.  Install a release that supports it.
RUN apt-get update && apt-get install -y --no-install-recommends unzip && \
    curl -LO https://github.com/protocolbuffers/protobuf/releases/download/v3.20.3/protoc-3.20.3-linux-x86_64.zip && \
    unzip protoc-3.20.3-linux-x86_64.zip -d /usr/local && \
    rm protoc-3.20.3-linux-x86_64.zip
COPY . $SRC/quickwit
WORKDIR $SRC/quickwit
COPY .clusterfuzzlite/build.sh $SRC/build.sh
