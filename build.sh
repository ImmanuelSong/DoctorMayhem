#!/bin/sh
# WSL에서 Windows exe 크로스 빌드. zig(링커)는 .tools/venv 에 있음: python3 -m venv .tools/venv && .tools/venv/bin/pip install ziglang cargo-zigbuild
cd "$(dirname "$0")" && python3 data/build_data.py && PATH="$PWD/.tools/venv/bin:$PATH" cargo zigbuild --release --target x86_64-pc-windows-gnu "$@"
