#!/usr/bin/env bash
set -euo pipefail

RID="${1:-}"
if [[ -z "${RID}" ]]; then
  echo "Usage: scripts/build-dotnet.sh <rid> [output-dir]" >&2
  echo "Example: scripts/build-dotnet.sh win-x64" >&2
  exit 1
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROJECT_PATH="${ROOT_DIR}/dotnet/GitSpace.Helper/GitSpace.Helper.csproj"
OUTPUT_DIR="${2:-${ROOT_DIR}/dist/dotnet/${RID}}"

mkdir -p "${OUTPUT_DIR}"

dotnet publish "${PROJECT_PATH}" \
  -c Release \
  -r "${RID}" \
  --self-contained true \
  /p:PublishSingleFile=true \
  /p:IncludeNativeLibrariesForSelfExtract=true \
  -o "${OUTPUT_DIR}"

echo "Published helper to ${OUTPUT_DIR}"
