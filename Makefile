UNAME_S := $(shell uname -s)

ifeq ($(UNAME_S),Darwin)
LIB_EXT := dylib
else ifeq ($(OS),Windows_NT)
LIB_EXT := dll
else
LIB_EXT := so
endif

LIB_NAME := target/release/libtiktoken_shim.$(LIB_EXT)

.PHONY: all build verify fmt tidy clean

all: build

build:
	cargo build --release

verify: build
	go run ./cmd/verify -lib $(LIB_NAME)

fmt:
	cargo fmt
	gofmt -w ./tiktokenffi ./cmd/verify

tidy:
	go mod tidy

clean:
	cargo clean
