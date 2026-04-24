package tiktoken

import (
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"testing"

	"github.com/Endgame-Labs/kesha/tiktokenffi"
)

func TestFindLibraryDoesNotLoadFromWorkingDirectory(t *testing.T) {
	t.Setenv(EnvLibraryPath, "")

	oldwd, err := os.Getwd()
	if err != nil {
		t.Fatal(err)
	}
	dir := t.TempDir()
	t.Cleanup(func() {
		if err := os.Chdir(oldwd); err != nil {
			t.Fatalf("restore working directory: %v", err)
		}
	})

	if err := os.Chdir(dir); err != nil {
		t.Fatal(err)
	}

	libName := tiktokenffi.DefaultLibName()
	writeEmptyFile(t, filepath.Join(dir, libName))
	writeEmptyFile(t, filepath.Join(dir, "target", "release", libName))

	got, err := FindLibrary()
	if err == nil {
		t.Fatalf("FindLibrary found %q from working directory; want no CWD fallback", got)
	}
}

func TestFindLibraryUsesExplicitEnvPath(t *testing.T) {
	path := filepath.Join(t.TempDir(), tiktokenffi.DefaultLibName())
	writeEmptyFile(t, path)
	t.Setenv(EnvLibraryPath, path)

	got, err := FindLibrary()
	if err != nil {
		t.Fatal(err)
	}
	if got != path {
		t.Fatalf("FindLibrary() = %q, want %q", got, path)
	}
}

func TestDefaultClientCachesSuccessfulOpenAndCanClose(t *testing.T) {
	resetDefaultStateForTest(t)

	openCalls := 0
	openDefaultClient = func() (*Client, error) {
		openCalls++
		return &Client{}, nil
	}

	first, err := DefaultClient()
	if err != nil {
		t.Fatal(err)
	}
	second, err := DefaultClient()
	if err != nil {
		t.Fatal(err)
	}
	if first != second {
		t.Fatal("DefaultClient returned different clients before CloseDefaultClient")
	}
	if openCalls != 1 {
		t.Fatalf("openDefaultClient called %d times, want 1", openCalls)
	}

	if err := CloseDefaultClient(); err != nil {
		t.Fatal(err)
	}
	third, err := DefaultClient()
	if err != nil {
		t.Fatal(err)
	}
	if third == first {
		t.Fatal("DefaultClient reused a client after CloseDefaultClient")
	}
	if openCalls != 2 {
		t.Fatalf("openDefaultClient called %d times after close, want 2", openCalls)
	}
}

func TestDefaultClientRetriesAfterOpenError(t *testing.T) {
	resetDefaultStateForTest(t)

	openCalls := 0
	openDefaultClient = func() (*Client, error) {
		openCalls++
		if openCalls == 1 {
			return nil, errors.New("missing library")
		}
		return &Client{}, nil
	}

	if _, err := DefaultClient(); err == nil {
		t.Fatal("DefaultClient succeeded on first open, want error")
	}
	if _, err := DefaultClient(); err != nil {
		t.Fatal(err)
	}
	if openCalls != 2 {
		t.Fatalf("openDefaultClient called %d times, want retry after error", openCalls)
	}
}

func TestPackageHelpersUseDefaultClientAndPropagateOptions(t *testing.T) {
	resetDefaultStateForTest(t)

	fake := &fakePackageClient{}
	defaultCalls := 0
	defaultPackageClient = func() (packageClient, error) {
		defaultCalls++
		return fake, nil
	}

	textOptions := TreatSpecialTokensAsText()
	count, err := CountWithOptions("gpt-4o", "<|endoftext|>", textOptions)
	if err != nil {
		t.Fatal(err)
	}
	if count != 42 {
		t.Fatalf("CountWithOptions returned %d, want fake count", count)
	}
	if fake.lastMethod != "CountWithOptions" || fake.lastModel != "gpt-4o" || fake.lastText != "<|endoftext|>" {
		t.Fatalf("CountWithOptions did not call fake client with expected args: %+v", fake)
	}
	if !reflect.DeepEqual(fake.lastOptions, textOptions) {
		t.Fatalf("CountWithOptions options = %+v, want %+v", fake.lastOptions, textOptions)
	}

	allowedOptions := EncodeOptions{AllowedSpecial: []string{"<|endoftext|>"}}
	tokens, err := EncodeWithEncoding("cl100k_base", "<|endoftext|>", allowedOptions)
	if err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(tokens, []uint32{1, 2, 3}) {
		t.Fatalf("EncodeWithEncoding returned %v, want fake tokens", tokens)
	}
	if fake.lastMethod != "EncodeWithEncoding" || fake.lastEncoding != "cl100k_base" {
		t.Fatalf("EncodeWithEncoding did not call fake client with expected args: %+v", fake)
	}
	if !reflect.DeepEqual(fake.lastOptions, allowedOptions) {
		t.Fatalf("EncodeWithEncoding options = %+v, want %+v", fake.lastOptions, allowedOptions)
	}

	if _, err := Count("gpt-4o", "hello"); err != nil {
		t.Fatal(err)
	}
	if _, err := Encode("gpt-4o", "hello"); err != nil {
		t.Fatal(err)
	}
	if _, err := Decode("gpt-4o", []uint32{15339}); err != nil {
		t.Fatal(err)
	}
	if defaultCalls != 5 {
		t.Fatalf("defaultPackageClient called %d times, want 5", defaultCalls)
	}
}

func TestTreatSpecialTokensAsTextOption(t *testing.T) {
	options := TreatSpecialTokensAsText()
	if options.Mode != SpecialModeEncodeAsText {
		t.Fatalf("TreatSpecialTokensAsText mode = %v, want %v", options.Mode, SpecialModeEncodeAsText)
	}
	if len(options.AllowedSpecial) != 0 {
		t.Fatalf("TreatSpecialTokensAsText allowed special = %v, want empty", options.AllowedSpecial)
	}
}

func writeEmptyFile(t *testing.T, path string) {
	t.Helper()

	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, nil, 0o644); err != nil {
		t.Fatal(err)
	}
}

func resetDefaultStateForTest(t *testing.T) {
	t.Helper()

	defaultMu.Lock()
	originalDefaultLib := defaultLib
	originalOpenDefaultClient := openDefaultClient
	originalDefaultPackageClient := defaultPackageClient
	defaultLib = nil
	openDefaultClient = OpenDefault
	defaultPackageClient = func() (packageClient, error) {
		return defaultClient()
	}
	defaultMu.Unlock()

	t.Cleanup(func() {
		defaultMu.Lock()
		defaultLib = originalDefaultLib
		openDefaultClient = originalOpenDefaultClient
		defaultPackageClient = originalDefaultPackageClient
		defaultMu.Unlock()
	})
}

type fakePackageClient struct {
	lastMethod   string
	lastModel    string
	lastEncoding string
	lastText     string
	lastTokens   []uint32
	lastOptions  EncodeOptions
}

func (f *fakePackageClient) Count(model, text string) (int, error) {
	f.lastMethod = "Count"
	f.lastModel = model
	f.lastText = text
	return 42, nil
}

func (f *fakePackageClient) CountWithOptions(model, text string, options EncodeOptions) (int, error) {
	f.lastMethod = "CountWithOptions"
	f.lastModel = model
	f.lastText = text
	f.lastOptions = options
	return 42, nil
}

func (f *fakePackageClient) CountWithEncoding(encoding, text string, options EncodeOptions) (int, error) {
	f.lastMethod = "CountWithEncoding"
	f.lastEncoding = encoding
	f.lastText = text
	f.lastOptions = options
	return 42, nil
}

func (f *fakePackageClient) Encode(model, text string) ([]uint32, error) {
	f.lastMethod = "Encode"
	f.lastModel = model
	f.lastText = text
	return []uint32{1, 2, 3}, nil
}

func (f *fakePackageClient) EncodeWithOptions(model, text string, options EncodeOptions) ([]uint32, error) {
	f.lastMethod = "EncodeWithOptions"
	f.lastModel = model
	f.lastText = text
	f.lastOptions = options
	return []uint32{1, 2, 3}, nil
}

func (f *fakePackageClient) EncodeWithEncoding(encoding, text string, options EncodeOptions) ([]uint32, error) {
	f.lastMethod = "EncodeWithEncoding"
	f.lastEncoding = encoding
	f.lastText = text
	f.lastOptions = options
	return []uint32{1, 2, 3}, nil
}

func (f *fakePackageClient) Decode(model string, tokens []uint32) (string, error) {
	f.lastMethod = "Decode"
	f.lastModel = model
	f.lastTokens = tokens
	return "hello", nil
}

func (f *fakePackageClient) DecodeWithEncoding(encoding string, tokens []uint32) (string, error) {
	f.lastMethod = "DecodeWithEncoding"
	f.lastEncoding = encoding
	f.lastTokens = tokens
	return "hello", nil
}
