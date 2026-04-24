// Package tiktoken provides a small Go API for counting, encoding, and decoding
// tokens with the upstream OpenAI tiktoken Rust implementation.
//
// Package-level helpers use a cached process-wide default client. Use
// TreatSpecialTokensAsText for raw content paths where strings like
// "<|endoftext|>" should be encoded as literal text.
package tiktoken
