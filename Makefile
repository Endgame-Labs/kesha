GOOS ?= $(shell go env GOOS)
GOARCH ?= $(shell go env GOARCH)

ifeq ($(GOOS),darwin)
LIB_EXT := dylib
LIB_BASENAME := libtiktoken_shim.$(LIB_EXT)
else ifeq ($(GOOS),windows)
LIB_EXT := dll
LIB_BASENAME := tiktoken_shim.$(LIB_EXT)
else
LIB_EXT := so
LIB_BASENAME := libtiktoken_shim.$(LIB_EXT)
endif

LIB_NAME := target/release/$(LIB_BASENAME)
DIST_DIR := dist

ifeq ($(GOOS),windows)
RELEASE_ASSET := tiktoken_shim_$(GOOS)_$(GOARCH).dll
else
RELEASE_ASSET := libtiktoken_shim_$(GOOS)_$(GOARCH).$(LIB_EXT)
endif

.PHONY: all build verify fmt fmt-check tidy lint lint-go lint-rust example package clean

all: build

build:
	cargo build --release --locked

verify: build
	go run ./cmd/verify -lib $(LIB_NAME)

fmt:
	cargo fmt
	gofmt -w ./tiktoken ./tiktokenffi ./cmd/verify ./cmd/kesha-install ./examples/go-app

fmt-check:
	cargo fmt -- --check
	test -z "$$(gofmt -l ./tiktoken ./tiktokenffi ./cmd/verify ./cmd/kesha-install ./examples/go-app)"

tidy:
	go mod tidy
	cd examples/go-app && go mod tidy

lint: lint-go lint-rust

lint-go:
	golangci-lint run ./...
	cd examples/go-app && golangci-lint run --config ../../.golangci.yml ./...

lint-rust:
	cargo clippy --locked --all-targets -- -D warnings

example: build
	cd examples/go-app && KESHA_TIKTOKEN_LIB=../../$(LIB_NAME) go run .

package: build
	mkdir -p $(DIST_DIR)
	cp $(LIB_NAME) $(DIST_DIR)/$(RELEASE_ASSET)
	if command -v shasum >/dev/null 2>&1; then \
		shasum -a 256 $(DIST_DIR)/$(RELEASE_ASSET) > $(DIST_DIR)/$(RELEASE_ASSET).sha256; \
	else \
		sha256sum $(DIST_DIR)/$(RELEASE_ASSET) > $(DIST_DIR)/$(RELEASE_ASSET).sha256; \
	fi

clean:
	cargo clean
	rm -rf $(DIST_DIR)
