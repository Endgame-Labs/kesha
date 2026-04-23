# tiktoken shim + example Go program to demonstrate usage. 

<img width="250" height="250" alt="image" src="https://github.com/user-attachments/assets/86482ee7-cfbe-4026-bd20-c5b0d24d7194" />

This allows Golang programs to access the actual rust library core that the OpenAI Tiktoken [library](https://github.com/openai/tiktoken) uses. 

This repo contains:

- a Rust `cdylib` shim built on top of upstream `openai/tiktoken`
- a Go `purego` wrapper in [`tiktokenffi`](./tiktokenffi)
- a verifier example in [`cmd/verify`](./cmd/verify)

## Build the shared library

```bash
cargo build --release
```

The shared library is written to:

- `target/release/libtiktoken_shim.dylib` on macOS
- `target/release/libtiktoken_shim.so` on Linux
- `target/release/tiktoken_shim.dll` on Windows

## Run the verifier

```bash
go run ./cmd/verify
```

You can also point the verifier at a different library path:

```bash
go run ./cmd/verify -lib /path/to/libtiktoken_shim.dylib
```

## Use From Another Go App

Build the shared library first:

```bash
make build
```

Then import the Go wrapper and pass the path to the built library:

```go
package main

import (
	"fmt"
	"log"

	"kesha/tiktokenffi"
)

func main() {
	lib, err := tiktokenffi.Open("/Users/rtyer/projects/endgame/kesha/target/release/libtiktoken_shim.dylib")
	if err != nil {
		log.Fatal(err)
	}
	defer lib.Close()

	count, err := lib.CountWithModel("gpt-4o", "hello world", tiktokenffi.EncodeOptions{})
	if err != nil {
		log.Fatal(err)
	}
	fmt.Println("count:", count)

	tokens, err := lib.EncodeWithEncoding("cl100k_base", "<|endoftext|>", tiktokenffi.EncodeOptions{
		Mode: tiktokenffi.SpecialModeEncodeAsText,
	})
	if err != nil {
		log.Fatal(err)
	}
	fmt.Println("tokens:", tokens)
}
```

While this repo is local-only, point your app at it with a `replace` directive:

```bash
go mod edit -require kesha@v0.0.0
go mod edit -replace kesha=/Users/rtyer/projects/endgame/kesha
go mod tidy
```

## Special token modes

The Go wrapper exposes four modes:

- `SpecialModeDisallow`: match upstream Python default and error on disallowed special tokens
- `SpecialModeEncodeAsText`: treat special-token text as ordinary text
- `SpecialModeAllowAll`: treat all known special tokens as special
- `SpecialModeAllowList`: only allow the explicitly provided special tokens
