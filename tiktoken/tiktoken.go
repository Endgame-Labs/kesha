package tiktoken

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"sync"

	"github.com/Endgame-Labs/kesha/tiktokenffi"
)

const EnvLibraryPath = "KESHA_TIKTOKEN_LIB"

type SpecialMode = tiktokenffi.SpecialMode

const (
	SpecialModeDisallow     = tiktokenffi.SpecialModeDisallow
	SpecialModeEncodeAsText = tiktokenffi.SpecialModeEncodeAsText
	SpecialModeAllowAll     = tiktokenffi.SpecialModeAllowAll
	SpecialModeAllowList    = tiktokenffi.SpecialModeAllowList
)

type EncodeOptions = tiktokenffi.EncodeOptions

type Client struct {
	lib *tiktokenffi.Library
}

func Open(path string) (*Client, error) {
	lib, err := tiktokenffi.Open(path)
	if err != nil {
		return nil, err
	}
	return &Client{lib: lib}, nil
}

func OpenDefault() (*Client, error) {
	path, err := FindLibrary()
	if err != nil {
		return nil, err
	}
	return Open(path)
}

func (c *Client) Close() error {
	if c == nil || c.lib == nil {
		return nil
	}
	return c.lib.Close()
}

func (c *Client) Version() (string, error) {
	if c == nil || c.lib == nil {
		return "", errors.New("tiktoken client is not initialized")
	}
	return c.lib.Version()
}

func (c *Client) EncodingNameForModel(model string) (string, error) {
	if c == nil || c.lib == nil {
		return "", errors.New("tiktoken client is not initialized")
	}
	return c.lib.EncodingNameForModel(model)
}

func (c *Client) Count(model, text string) (int, error) {
	return c.CountWithOptions(model, text, EncodeOptions{})
}

func (c *Client) CountWithOptions(model, text string, options EncodeOptions) (int, error) {
	if c == nil || c.lib == nil {
		return 0, errors.New("tiktoken client is not initialized")
	}
	count, err := c.lib.CountWithModel(model, text, options)
	if err != nil {
		return 0, err
	}
	return int(count), nil
}

func (c *Client) CountWithEncoding(encoding, text string, options EncodeOptions) (int, error) {
	if c == nil || c.lib == nil {
		return 0, errors.New("tiktoken client is not initialized")
	}
	count, err := c.lib.CountWithEncoding(encoding, text, options)
	if err != nil {
		return 0, err
	}
	return int(count), nil
}

func (c *Client) Encode(model, text string) ([]uint32, error) {
	return c.EncodeWithOptions(model, text, EncodeOptions{})
}

func (c *Client) EncodeWithOptions(model, text string, options EncodeOptions) ([]uint32, error) {
	if c == nil || c.lib == nil {
		return nil, errors.New("tiktoken client is not initialized")
	}
	return c.lib.EncodeWithModel(model, text, options)
}

func (c *Client) EncodeWithEncoding(encoding, text string, options EncodeOptions) ([]uint32, error) {
	if c == nil || c.lib == nil {
		return nil, errors.New("tiktoken client is not initialized")
	}
	return c.lib.EncodeWithEncoding(encoding, text, options)
}

func (c *Client) Decode(model string, tokens []uint32) (string, error) {
	if c == nil || c.lib == nil {
		return "", errors.New("tiktoken client is not initialized")
	}
	return c.lib.DecodeWithModel(model, tokens)
}

func (c *Client) DecodeWithEncoding(encoding string, tokens []uint32) (string, error) {
	if c == nil || c.lib == nil {
		return "", errors.New("tiktoken client is not initialized")
	}
	return c.lib.DecodeWithEncoding(encoding, tokens)
}

func Count(model, text string) (int, error) {
	client, err := defaultClient()
	if err != nil {
		return 0, err
	}
	return client.Count(model, text)
}

func CountWithOptions(model, text string, options EncodeOptions) (int, error) {
	client, err := defaultClient()
	if err != nil {
		return 0, err
	}
	return client.CountWithOptions(model, text, options)
}

func CountWithEncoding(encoding, text string, options EncodeOptions) (int, error) {
	client, err := defaultClient()
	if err != nil {
		return 0, err
	}
	return client.CountWithEncoding(encoding, text, options)
}

func Encode(model, text string) ([]uint32, error) {
	client, err := defaultClient()
	if err != nil {
		return nil, err
	}
	return client.Encode(model, text)
}

func EncodeWithOptions(model, text string, options EncodeOptions) ([]uint32, error) {
	client, err := defaultClient()
	if err != nil {
		return nil, err
	}
	return client.EncodeWithOptions(model, text, options)
}

func EncodeWithEncoding(encoding, text string, options EncodeOptions) ([]uint32, error) {
	client, err := defaultClient()
	if err != nil {
		return nil, err
	}
	return client.EncodeWithEncoding(encoding, text, options)
}

func Decode(model string, tokens []uint32) (string, error) {
	client, err := defaultClient()
	if err != nil {
		return "", err
	}
	return client.Decode(model, tokens)
}

func DecodeWithEncoding(encoding string, tokens []uint32) (string, error) {
	client, err := defaultClient()
	if err != nil {
		return "", err
	}
	return client.DecodeWithEncoding(encoding, tokens)
}

func FindLibrary() (string, error) {
	if path := os.Getenv(EnvLibraryPath); path != "" {
		if fileExists(path) {
			return path, nil
		}
		return "", fmt.Errorf("%s points to missing file %q", EnvLibraryPath, path)
	}

	libName := tiktokenffi.DefaultLibName()
	var candidates []string

	if exePath, err := os.Executable(); err == nil {
		exeDir := filepath.Dir(exePath)
		candidates = append(candidates,
			filepath.Join(exeDir, libName),
			filepath.Join(exeDir, "lib", libName),
		)
	}

	if cacheDir, err := os.UserCacheDir(); err == nil {
		candidates = append(candidates, filepath.Join(cacheDir, "kesha", libName))
	}

	for _, candidate := range candidates {
		if fileExists(candidate) {
			return candidate, nil
		}
	}

	return "", fmt.Errorf("could not find %s for %s/%s; set %s to a trusted absolute path or run: go run github.com/Endgame-Labs/kesha/cmd/kesha-install@latest",
		libName, runtime.GOOS, runtime.GOARCH, EnvLibraryPath)
}

var (
	defaultMu     sync.Mutex
	defaultLib    *Client
	defaultLibErr error
)

func defaultClient() (*Client, error) {
	defaultMu.Lock()
	defer defaultMu.Unlock()

	if defaultLib != nil || defaultLibErr != nil {
		return defaultLib, defaultLibErr
	}

	defaultLib, defaultLibErr = OpenDefault()
	return defaultLib, defaultLibErr
}

func fileExists(path string) bool {
	info, err := os.Stat(path)
	return err == nil && !info.IsDir()
}
