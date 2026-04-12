FROM gcr.io/oss-fuzz-base/base-builder-rust
RUN apt-get update && apt-get install -y --no-install-recommends protobuf-compiler
COPY . $SRC/quickwit
WORKDIR $SRC/quickwit
COPY .clusterfuzzlite/build.sh $SRC/build.sh
