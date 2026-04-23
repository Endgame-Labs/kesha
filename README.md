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

## Special token modes

The Go wrapper exposes four modes:

- `SpecialModeDisallow`: match upstream Python default and error on disallowed special tokens
- `SpecialModeEncodeAsText`: treat special-token text as ordinary text
- `SpecialModeAllowAll`: treat all known special tokens as special
- `SpecialModeAllowList`: only allow the explicitly provided special tokens
