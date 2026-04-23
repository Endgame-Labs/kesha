# Kesha

Go bindings for token counting and encoding through the upstream OpenAI
[`tiktoken`](https://github.com/openai/tiktoken) Rust core.

Kesha keeps the Go side CGO-free by loading a small Rust shared library with
[`purego`](https://github.com/ebitengine/purego). The Rust shim embeds the
official `.tiktoken` vocab files, so normal runtime use does not download
tokenizer assets.

## Quickstart

Install the native library for a supported platform:

```bash
go run github.com/Endgame-Labs/kesha/cmd/kesha-install@latest
```

From your Go app's module directory, add Kesha:

```bash
go get github.com/Endgame-Labs/kesha
```

Use the high-level package:

```go
package main

import (
	"fmt"
	"log"

	"github.com/Endgame-Labs/kesha/tiktoken"
)

func main() {
	count, err := tiktoken.Count("gpt-4o", "hello world")
	if err != nil {
		log.Fatal(err)
	}
	fmt.Println(count)
}
```

If your app cannot find the native library automatically, set:

```bash
export KESHA_TIKTOKEN_LIB=/path/to/libtiktoken_shim.so
```

On macOS the library is `libtiktoken_shim.dylib`; on Windows it is
`tiktoken_shim.dll`.

For safety, default discovery does not load native libraries from the current
working directory. Local development commands pass explicit paths to the shared
library instead.

Supported release artifacts currently cover:

- `darwin/arm64`
- `linux/amd64`
- `windows/amd64`

Other platforms can still build from source, but `kesha-install` will only work
once a matching release artifact exists.

## Why An Installer?

Most Go modules work with only `go get`. Kesha has one extra install step
because it loads the upstream Rust tokenizer through a small native shared
library. That keeps consuming Go builds CGO-free while avoiding tokenizer drift
from a Go port.

The installer is explicit by design: Kesha does not silently download native
code at application runtime. Future releases may add embedded platform packages
for a more automatic Go experience, but the current model keeps the native
library install visible and verifiable.

## API Examples

Create an explicit client when you want to control the library path or
lifecycle:

```go
client, err := tiktoken.Open("/usr/local/lib/libtiktoken_shim.so")
if err != nil {
	log.Fatal(err)
}
defer client.Close()

tokens, err := client.Encode("gpt-4o", "hello world")
if err != nil {
	log.Fatal(err)
}
fmt.Println(tokens)
```

Use an encoding directly:

```go
count, err := tiktoken.CountWithEncoding("cl100k_base", "hello world", tiktoken.EncodeOptions{})
```

Control special-token behavior:

```go
tokens, err := tiktoken.EncodeWithOptions("gpt-4", "<|endoftext|>", tiktoken.EncodeOptions{
	Mode: tiktoken.SpecialModeEncodeAsText,
})
```

The default mode matches upstream Python `tiktoken` and errors when disallowed
special-token text is present.

## Local Development

Build the Rust shared library:

```bash
make build
```

For ad hoc local apps, point `KESHA_TIKTOKEN_LIB` at the built library:

```bash
export KESHA_TIKTOKEN_LIB="$PWD/target/release/libtiktoken_shim.dylib"
```

Run the end-to-end verifier:

```bash
make verify
```

Run the normal checks:

```bash
make fmt-check
make lint
cargo test --locked
go test ./...
go test -race ./...
```

Run the standalone example app:

```bash
make example
```

Package the native library using the same asset naming used by GitHub Releases:

```bash
make package
```

## Low-Level FFI

Most Go apps should import `github.com/Endgame-Labs/kesha/tiktoken`.

The lower-level package `github.com/Endgame-Labs/kesha/tiktokenffi` exposes the
raw `purego` wrapper around the C ABI. Use it when you need manual library
loading or direct access to the shim functions.

## Release Artifacts

Tagged releases build native libraries for supported platforms and publish
assets named like:

- `libtiktoken_shim_linux_amd64.so`
- `libtiktoken_shim_darwin_arm64.dylib`
- `tiktoken_shim_windows_amd64.dll`

Each native library has a `.sha256` sidecar. `cmd/kesha-install` downloads the
matching artifact for the current platform, verifies the checksum, and installs
it into the user cache.

## Versioning

The current shim is built against upstream `openai/tiktoken` `0.12.0`.

## License

Kesha is licensed under the MIT License. See [LICENSE](./LICENSE).
