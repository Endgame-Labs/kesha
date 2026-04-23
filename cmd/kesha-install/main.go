package main

import (
	"crypto/sha256"
	"encoding/hex"
	"flag"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"time"

	"github.com/Endgame-Labs/kesha/tiktokenffi"
)

func main() {
	repo := flag.String("repo", "Endgame-Labs/kesha", "GitHub owner/repo to download from")
	version := flag.String("version", "latest", "release version, or latest")
	dir := flag.String("dir", defaultInstallDir(), "directory to install the native library into")
	flag.Parse()

	if err := install(*repo, *version, *dir); err != nil {
		_, _ = fmt.Fprintf(os.Stderr, "kesha-install: %v\n", err)
		os.Exit(1)
	}
}

func install(repo, version, dir string) error {
	asset := releaseAssetName()
	downloadURL := releaseURL(repo, version, asset)
	checksumURL := downloadURL + ".sha256"

	client := &http.Client{Timeout: 2 * time.Minute}
	expected, err := downloadChecksum(client, checksumURL)
	if err != nil {
		return err
	}

	data, err := download(client, downloadURL)
	if err != nil {
		return err
	}
	if got := sha256Hex(data); got != expected {
		return fmt.Errorf("checksum mismatch for %s: got %s, want %s", asset, got, expected)
	}

	if err := os.MkdirAll(dir, 0o755); err != nil {
		return fmt.Errorf("create install dir: %w", err)
	}

	target := filepath.Join(dir, tiktokenffi.DefaultLibName())
	tmp := target + ".tmp"
	if err := os.WriteFile(tmp, data, 0o755); err != nil {
		return fmt.Errorf("write temp library: %w", err)
	}
	if err := os.Rename(tmp, target); err != nil {
		_ = os.Remove(tmp)
		return fmt.Errorf("install library: %w", err)
	}

	fmt.Printf("installed %s\n", target)
	fmt.Printf("set %s=%s if your app cannot find it automatically\n", "KESHA_TIKTOKEN_LIB", target)
	return nil
}

func defaultInstallDir() string {
	cacheDir, err := os.UserCacheDir()
	if err != nil {
		return "."
	}
	return filepath.Join(cacheDir, "kesha")
}

func releaseAssetName() string {
	switch runtime.GOOS {
	case "darwin":
		return fmt.Sprintf("libtiktoken_shim_%s_%s.dylib", runtime.GOOS, runtime.GOARCH)
	case "windows":
		return fmt.Sprintf("tiktoken_shim_%s_%s.dll", runtime.GOOS, runtime.GOARCH)
	default:
		return fmt.Sprintf("libtiktoken_shim_%s_%s.so", runtime.GOOS, runtime.GOARCH)
	}
}

func releaseURL(repo, version, asset string) string {
	if version == "latest" {
		return fmt.Sprintf("https://github.com/%s/releases/latest/download/%s", repo, asset)
	}
	return fmt.Sprintf("https://github.com/%s/releases/download/%s/%s", repo, version, asset)
}

func downloadChecksum(client *http.Client, url string) (string, error) {
	data, err := download(client, url)
	if err != nil {
		return "", err
	}
	fields := strings.Fields(string(data))
	if len(fields) == 0 {
		return "", fmt.Errorf("empty checksum file %s", url)
	}
	checksum := strings.ToLower(fields[0])
	if _, err := hex.DecodeString(checksum); err != nil || len(checksum) != sha256.Size*2 {
		return "", fmt.Errorf("invalid checksum %q from %s", checksum, url)
	}
	return checksum, nil
}

func download(client *http.Client, url string) ([]byte, error) {
	resp, err := client.Get(url)
	if err != nil {
		return nil, fmt.Errorf("download %s: %w", url, err)
	}
	defer func() {
		_ = resp.Body.Close()
	}()

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("download %s: %s", url, resp.Status)
	}

	data, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, fmt.Errorf("read %s: %w", url, err)
	}
	return data, nil
}

func sha256Hex(data []byte) string {
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:])
}
