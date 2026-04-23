package tiktoken

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/Endgame-Labs/kesha/tiktokenffi"
)

func TestFindLibraryDoesNotLoadFromWorkingDirectory(t *testing.T) {
	t.Setenv(EnvLibraryPath, "")

	oldwd, err := os.Getwd()
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := os.Chdir(oldwd); err != nil {
			t.Fatalf("restore working directory: %v", err)
		}
	})

	dir := t.TempDir()
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

func writeEmptyFile(t *testing.T, path string) {
	t.Helper()

	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, nil, 0o644); err != nil {
		t.Fatal(err)
	}
}
