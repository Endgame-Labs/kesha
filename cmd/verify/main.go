package main

import (
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"kesha/tiktokenffi"
)

type encodeCase struct {
	label         string
	text          string
	useModel      bool
	name          string
	options       tiktokenffi.EncodeOptions
	wantTokens    []uint32
	wantCount     uint64
	wantErrSubstr string
}

func main() {
	libPath := flag.String("lib", filepath.Join("target", "release", tiktokenffi.DefaultLibName()), "path to the Rust shared library")
	flag.Parse()

	lib, err := tiktokenffi.Open(*libPath)
	if err != nil {
		fatalf("open library: %v", err)
	}
	defer func() {
		if closeErr := lib.Close(); closeErr != nil {
			fatalf("close library: %v", closeErr)
		}
	}()

	version, err := lib.Version()
	if err != nil {
		fatalf("read version: %v", err)
	}
	fmt.Printf("Loaded %s from %s\n", version, *libPath)

	modelEncoding, err := lib.EncodingNameForModel("gpt-4o")
	if err != nil {
		fatalf("resolve model encoding: %v", err)
	}
	fmt.Printf("gpt-4o -> %s\n", modelEncoding)

	cases := []encodeCase{
		{
			label:      "cl100k ordinary",
			name:       "cl100k_base",
			text:       "hello world",
			wantTokens: []uint32{15339, 1917},
			wantCount:  2,
		},
		{
			label:      "o200k ordinary",
			name:       "o200k_base",
			text:       "hello world",
			wantTokens: []uint32{24912, 2375},
			wantCount:  2,
		},
		{
			label:         "cl100k disallow special",
			name:          "cl100k_base",
			text:          "<|endoftext|>",
			wantErrSubstr: "disallowed special token",
		},
		{
			label:      "cl100k special as text",
			name:       "cl100k_base",
			text:       "<|endoftext|>",
			options:    tiktokenffi.EncodeOptions{Mode: tiktokenffi.SpecialModeEncodeAsText},
			wantTokens: []uint32{27, 91, 8862, 728, 428, 91, 29},
			wantCount:  7,
		},
		{
			label:      "cl100k special as token",
			name:       "cl100k_base",
			text:       "<|endoftext|>",
			options:    tiktokenffi.EncodeOptions{Mode: tiktokenffi.SpecialModeAllowAll},
			wantTokens: []uint32{100257},
			wantCount:  1,
		},
		{
			label:      "gpt-4o by model",
			name:       "gpt-4o",
			useModel:   true,
			text:       "hello world",
			wantTokens: []uint32{24912, 2375},
			wantCount:  2,
		},
	}

	for _, tc := range cases {
		if err := runCase(lib, tc); err != nil {
			fatalf("%s: %v", tc.label, err)
		}
	}

	fmt.Println("verification passed")
}

func runCase(lib *tiktokenffi.Library, tc encodeCase) error {
	var (
		tokens []uint32
		count  uint64
		err    error
	)

	if tc.useModel {
		tokens, err = lib.EncodeWithModel(tc.name, tc.text, tc.options)
		if err == nil {
			count, err = lib.CountWithModel(tc.name, tc.text, tc.options)
		}
	} else {
		tokens, err = lib.EncodeWithEncoding(tc.name, tc.text, tc.options)
		if err == nil {
			count, err = lib.CountWithEncoding(tc.name, tc.text, tc.options)
		}
	}

	if tc.wantErrSubstr != "" {
		if err == nil {
			return fmt.Errorf("expected error containing %q, got success", tc.wantErrSubstr)
		}
		if !strings.Contains(err.Error(), tc.wantErrSubstr) {
			return fmt.Errorf("expected error containing %q, got %q", tc.wantErrSubstr, err.Error())
		}
		fmt.Printf("ok  %-24s error=%q\n", tc.label, err.Error())
		return nil
	}

	if err != nil {
		return err
	}
	if count != uint64(len(tokens)) {
		return fmt.Errorf("count/token mismatch: count=%d len(tokens)=%d", count, len(tokens))
	}
	if tc.wantTokens != nil && !equalTokens(tokens, tc.wantTokens) {
		return fmt.Errorf("unexpected tokens: got=%v want=%v", tokens, tc.wantTokens)
	}
	if tc.wantCount != 0 && count != tc.wantCount {
		return fmt.Errorf("unexpected count: got=%d want=%d", count, tc.wantCount)
	}

	fmt.Printf("ok  %-24s count=%d tokens=%v\n", tc.label, count, tokens)
	return nil
}

func equalTokens(got, want []uint32) bool {
	if len(got) != len(want) {
		return false
	}
	for i := range got {
		if got[i] != want[i] {
			return false
		}
	}
	return true
}

func fatalf(format string, args ...any) {
	_, _ = fmt.Fprintf(os.Stderr, format+"\n", args...)
	os.Exit(1)
}
