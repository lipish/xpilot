#!/bin/bash

# Input variables
TABBY_VERSION=${TABBY_VERSION:-dev}
OUTPUT_NAME=${OUTPUT_NAME:-tabby_${TABBY_VERSION}_x86_64-windows-msvc}

mkdir -p dist
mv target/release/tabby.exe dist/${OUTPUT_NAME}.exe