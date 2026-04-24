package main

import (
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"flag"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"runtime"
	"runtime/debug"
	"strings"
	"time"
)

const githubBaseURL = "https://github.com"

func main() {
	repo := flag.String("repo", "Endgame-Labs/kesha", "GitHub owner/repo to download from")
	version := flag.String("version", "auto", "release version, latest, or auto to use the module version from go run ...@vX")
	targetOS := flag.String("os", runtime.GOOS, "target GOOS release asset to install")
	targetArch := flag.String("arch", runtime.GOARCH, "target GOARCH release asset to install")
	dir := flag.String("dir", defaultInstallDir(), "directory to install the native library into")
	output := flag.String("output", "", "full path to write the native library; overrides -dir")
	flag.Parse()

	options := installOptions{
		Repo:    *repo,
		Version: *version,
		GOOS:    *targetOS,
		GOARCH:  *targetArch,
		Dir:     *dir,
		Output:  *output,
	}
	if err := install(options); err != nil {
		_, _ = fmt.Fprintf(os.Stderr, "kesha-install: %v\n", err)
		os.Exit(1)
	}
}

type installOptions struct {
	Repo    string
	Version string
	GOOS    string
	GOARCH  string
	Dir     string
	Output  string
}

func install(options installOptions) error {
	client := &http.Client{Timeout: 2 * time.Minute}
	return installer{
		baseURL: githubBaseURL,
		client:  client,
	}.install(options)
}

type installer struct {
	baseURL string
	client  *http.Client
}

func (i installer) install(options installOptions) error {
	if options.Repo == "" {
		return errors.New("repo cannot be empty")
	}
	if options.GOOS == "" {
		return errors.New("os cannot be empty")
	}
	if options.GOARCH == "" {
		return errors.New("arch cannot be empty")
	}

	version := resolveReleaseVersion(options.Version)
	asset, err := releaseAssetName(options.GOOS, options.GOARCH)
	if err != nil {
		return err
	}
	downloadURL := releaseURL(i.baseURL, options.Repo, version, asset)
	checksumURL := downloadURL + ".sha256"

	expected, err := downloadChecksum(i.client, checksumURL)
	if err != nil {
		return err
	}

	data, err := download(i.client, downloadURL)
	if err != nil {
		return err
	}
	if got := sha256Hex(data); got != expected {
		return fmt.Errorf("checksum mismatch for %s: got %s, want %s", asset, got, expected)
	}

	target, err := installTarget(options)
	if err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Dir(target), 0o755); err != nil {
		return fmt.Errorf("create install dir: %w", err)
	}

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

func resolveReleaseVersion(version string) string {
	if version != "" && version != "auto" {
		return version
	}
	if moduleVersion := currentModuleVersion(); moduleVersion != "" {
		return moduleVersion
	}
	return "latest"
}

func currentModuleVersion() string {
	info, ok := debug.ReadBuildInfo()
	if !ok {
		return ""
	}
	version := info.Main.Version
	if version == "" || version == "(devel)" {
		return ""
	}
	return version
}

func defaultInstallDir() string {
	cacheDir, err := os.UserCacheDir()
	if err != nil {
		return "."
	}
	return filepath.Join(cacheDir, "kesha")
}

func installTarget(options installOptions) (string, error) {
	if options.Output != "" {
		return options.Output, nil
	}
	if options.Dir == "" {
		return "", errors.New("dir cannot be empty when output is not set")
	}
	libName, err := defaultLibName(options.GOOS)
	if err != nil {
		return "", err
	}
	return filepath.Join(options.Dir, libName), nil
}

func defaultLibName(goos string) (string, error) {
	switch goos {
	case "darwin":
		return "libtiktoken_shim.dylib", nil
	case "windows":
		return "tiktoken_shim.dll", nil
	default:
		return "libtiktoken_shim.so", nil
	}
}

func releaseAssetName(goos, goarch string) (string, error) {
	if goarch == "" {
		return "", errors.New("arch cannot be empty")
	}

	switch goos {
	case "darwin":
		return fmt.Sprintf("libtiktoken_shim_%s_%s.dylib", goos, goarch), nil
	case "windows":
		return fmt.Sprintf("tiktoken_shim_%s_%s.dll", goos, goarch), nil
	case "linux":
		return fmt.Sprintf("libtiktoken_shim_%s_%s.so", goos, goarch), nil
	default:
		return "", fmt.Errorf("unsupported os %q", goos)
	}
}

func releaseURL(baseURL, repo, version, asset string) string {
	if version == "latest" {
		return fmt.Sprintf("%s/%s/releases/latest/download/%s", strings.TrimRight(baseURL, "/"), repo, asset)
	}
	return fmt.Sprintf("%s/%s/releases/download/%s/%s", strings.TrimRight(baseURL, "/"), repo, version, asset)
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
