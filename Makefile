SHELL := /bin/bash

CARGO ?= cargo
WORKSPACE_FLAGS ?= --workspace

X86_TARGET ?= x86_64-unknown-linux-gnu
ARM_TARGET ?= aarch64-unknown-linux-gnu

# Override this if your cross-linker path/name differs.
ARM_LINKER ?= aarch64-linux-gnu-gcc

# Generic release tuning.
BASE_RELEASE_RUSTFLAGS ?= -C debuginfo=0

# Raspberry Pi 5 (Cortex-A76) release tuning.
PI5_RUSTFLAGS ?= -C target-cpu=cortex-a76 -C target-feature=+neon,+crc,+crypto -C lto=thin -C codegen-units=1 -C debuginfo=0

.PHONY: help setup-targets proto build build-x86 build-arm build-arm-pi5 build-all clean

help:
	@echo "Available targets:"
	@echo "  make setup-targets   # install x86_64 + aarch64 rust targets"
	@echo "  make proto           # rebuild protobuf/gRPC generated code only"
	@echo "  make build           # release build for host target"
	@echo "  make build-x86       # release build for x86_64-unknown-linux-gnu"
	@echo "  make build-arm       # generic ARM64 release build"
	@echo "  make build-arm-pi5   # ARM64 release build optimized for Raspberry Pi 5"
	@echo "  make build-all       # x86 + ARM + ARM Pi5 builds"
	@echo "  make clean           # remove build artifacts"
	@echo ""
	@echo "Defaults:"
	@echo "  CARGO_INCREMENTAL=1  # enabled for faster local rebuilds"

setup-targets:
	rustup target add $(X86_TARGET) $(ARM_TARGET)

proto:
	$(CARGO) clean -p proto
	$(CARGO) check -p proto

build:
	CARGO_INCREMENTAL=1 $(CARGO) build $(WORKSPACE_FLAGS) --release

build-x86:
	CARGO_INCREMENTAL=1 \
	RUSTFLAGS="$(BASE_RELEASE_RUSTFLAGS)" \
		$(CARGO) build $(WORKSPACE_FLAGS) --release --target $(X86_TARGET)

build-arm:
	CARGO_INCREMENTAL=1 \
	CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=$(ARM_LINKER) \
	RUSTFLAGS="$(BASE_RELEASE_RUSTFLAGS)" \
		$(CARGO) build $(WORKSPACE_FLAGS) --release --target $(ARM_TARGET)

build-arm-pi5:
	CARGO_INCREMENTAL=1 \
	CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=$(ARM_LINKER) \
	RUSTFLAGS="$(PI5_RUSTFLAGS)" \
		$(CARGO) build $(WORKSPACE_FLAGS) --release --target $(ARM_TARGET)

build-all: build-x86 build-arm build-arm-pi5

clean:
	$(CARGO) clean
