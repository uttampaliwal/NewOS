This folder contains Docker files for building Turnix on Linux from Windows.

Use:
  docker build -t turnix-build -f docker/Dockerfile.build .
  docker run --rm -v "$(pwd)":/work -w /work turnix-build
