# Minimal Executables

This directory contains assembly code for ARM64 and x86-64 that demonstrates how to create two minimal executables: an exit program that simply returns 42, and a hello world program that prints "Hello, world!" to the console.

# Building and running

In order to build and run these ELF executables you will need a Linux environment with the appropriate toolchain.

## Apple Silicon Mac workflow (volume mount by default)

If you are on a Mac, you will not have the native Linux toolchain (macOS uses Mach-O instead of ELF), but you can use Docker or Podman to run a Linux container. Either a native ARM64 Linux container or an emulated x86-64 Linux container.

Using a bind mount so your local edits are immediately visible in the container:

```bash
# ARM64 Linux container
docker run --rm -it -v "$(pwd)":/work -w /work arm64v8/ubuntu bash
podman run --rm -it -v "$(pwd)":/work -w /work docker.io/arm64v8/ubuntu bash

# x86-64 Linux container (emulated)
docker run --rm -it --platform linux/amd64 -v "$(pwd)":/work -w /work ubuntu:24.04 bash
podman run --rm -it --arch amd64 -v "$(pwd)":/work -w /work docker.io/library/ubuntu:24.04 bash

# Inside the container:
apt-get update && apt-get install -y binutils nasm file strace make
# You might also want some utilities such as a text editor
```

### Alternative: copy files in (docker cp / podman cp)

For a snapshot of files at copy time instead of a live mount.

```bash
# Docker
docker create --name elf-build --platform linux/amd64 -it ubuntu:24.04 bash
docker cp . elf-build:/work
docker start -ai elf-build

# Podman
podman create --name elf-build --arch amd64 -it docker.io/library/ubuntu:24.04 bash
podman cp . elf-build:/work
podman start -ai elf-build

# Inside the container:
cd /work
apt-get update && apt-get install -y binutils nasm file strace
```

