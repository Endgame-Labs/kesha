# Contributing

Thanks for helping improve Kesha.

## Local Setup

Build the native shim:

```bash
make build
```

Run the verifier:

```bash
make verify
```

Run the full local check set before opening a pull request:

```bash
make fmt
cargo test --locked
go test ./...
go test -race ./...
```

## Compatibility Expectations

Kesha aims to match upstream OpenAI `tiktoken` behavior for supported
encodings. Changes that affect tokenization should include verifier cases for:

- Token IDs, not only counts.
- Model-to-encoding resolution when relevant.
- Special-token behavior, including disallowed and explicitly allowed cases.

## Releases

Tagged releases publish native libraries for supported platforms. Release assets
must keep the naming convention used by `cmd/kesha-install`:

- `libtiktoken_shim_linux_amd64.so`
- `libtiktoken_shim_darwin_arm64.dylib`
- `tiktoken_shim_windows_amd64.dll`

Each asset must have a matching `.sha256` sidecar.
