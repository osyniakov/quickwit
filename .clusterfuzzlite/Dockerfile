FROM gcr.io/oss-fuzz-base/base-builder-rust
COPY . $SRC/quickwit
WORKDIR $SRC/quickwit
COPY .clusterfuzzlite/build.sh $SRC/build.sh
