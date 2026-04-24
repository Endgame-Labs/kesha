package main

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"
)

func TestInstallDownloadsRequestedPlatformToOutputAndVerifiesChecksum(t *testing.T) {
	asset := "libtiktoken_shim_linux_amd64.so"
	data := []byte("native library bytes")
	checksum := sha256Hex(data)

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/Endgame-Labs/kesha/releases/download/v1.2.3/" + asset:
			_, _ = w.Write(data)
		case "/Endgame-Labs/kesha/releases/download/v1.2.3/" + asset + ".sha256":
			_, _ = fmt.Fprintf(w, "%s  %s\n", checksum, asset)
		default:
			http.NotFound(w, r)
		}
	}))
	t.Cleanup(server.Close)

	output := filepath.Join(t.TempDir(), "lib", "libtiktoken_shim.so")
	err := installer{
		baseURL: server.URL,
		client:  server.Client(),
	}.install(installOptions{
		Repo:    "Endgame-Labs/kesha",
		Version: "v1.2.3",
		GOOS:    "linux",
		GOARCH:  "amd64",
		Output:  output,
	})
	if err != nil {
		t.Fatal(err)
	}

	got, err := os.ReadFile(output)
	if err != nil {
		t.Fatal(err)
	}
	if string(got) != string(data) {
		t.Fatalf("installed data = %q, want %q", got, data)
	}
}

func TestInstallRejectsChecksumMismatch(t *testing.T) {
	asset := "libtiktoken_shim_darwin_arm64.dylib"
	data := []byte("native library bytes")
	badChecksum := hex.EncodeToString(make([]byte, sha256.Size))

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/Endgame-Labs/kesha/releases/download/v1.2.3/" + asset:
			_, _ = w.Write(data)
		case "/Endgame-Labs/kesha/releases/download/v1.2.3/" + asset + ".sha256":
			_, _ = fmt.Fprintln(w, badChecksum)
		default:
			http.NotFound(w, r)
		}
	}))
	t.Cleanup(server.Close)

	output := filepath.Join(t.TempDir(), "libtiktoken_shim.dylib")
	err := installer{
		baseURL: server.URL,
		client:  server.Client(),
	}.install(installOptions{
		Repo:    "Endgame-Labs/kesha",
		Version: "v1.2.3",
		GOOS:    "darwin",
		GOARCH:  "arm64",
		Output:  output,
	})
	if err == nil {
		t.Fatal("install succeeded with checksum mismatch, want error")
	}
	if _, statErr := os.Stat(output); !os.IsNotExist(statErr) {
		t.Fatalf("output exists after checksum mismatch: %v", statErr)
	}
}

func TestReleaseAssetNameForPlatform(t *testing.T) {
	tests := []struct {
		goos   string
		goarch string
		want   string
	}{
		{goos: "darwin", goarch: "arm64", want: "libtiktoken_shim_darwin_arm64.dylib"},
		{goos: "linux", goarch: "amd64", want: "libtiktoken_shim_linux_amd64.so"},
		{goos: "windows", goarch: "amd64", want: "tiktoken_shim_windows_amd64.dll"},
	}

	for _, test := range tests {
		got, err := releaseAssetName(test.goos, test.goarch)
		if err != nil {
			t.Fatal(err)
		}
		if got != test.want {
			t.Fatalf("releaseAssetName(%q, %q) = %q, want %q", test.goos, test.goarch, got, test.want)
		}
	}
}

func TestReleaseURLUsesLatestOrExactVersion(t *testing.T) {
	exact := releaseURL("https://github.example/", "owner/repo", "v1.2.3", "asset.so")
	if exact != "https://github.example/owner/repo/releases/download/v1.2.3/asset.so" {
		t.Fatalf("exact release URL = %q", exact)
	}

	latest := releaseURL("https://github.example/", "owner/repo", "latest", "asset.so")
	if latest != "https://github.example/owner/repo/releases/latest/download/asset.so" {
		t.Fatalf("latest release URL = %q", latest)
	}
}
