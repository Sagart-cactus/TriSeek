#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE="${TRISEEK_HARNESS_IMAGE:-triseek-real-harness:local}"
DOCKERFILE="${ROOT}/docker/real-harness.Dockerfile"

docker build -f "${DOCKERFILE}" -t "${IMAGE}" "${ROOT}"

docker_args=(
  run
  --rm
  --user "$(id -u):$(id -g)"
  -e CARGO_HOME=/tmp/cargo-home
  -e CARGO_TARGET_DIR=/tmp/triseek-target
  -e TRISEEK_HARNESS_IN_DOCKER=1
  -v "${ROOT}:/workspace"
  -w /workspace
)

if [[ -n "${TRISEEK_LARGE_REPO:-}" ]]; then
  large_repo_abs="$(cd "${TRISEEK_LARGE_REPO}" && pwd)"
  docker_args+=(
    -e TRISEEK_LARGE_REPO=/mnt/triseek-large-repo
    -v "${large_repo_abs}:/mnt/triseek-large-repo:ro"
  )
fi

docker "${docker_args[@]}" "${IMAGE}" /workspace/scripts/real_harness.sh "$@"
