package tiktokenffi

import (
	"path/filepath"
	"testing"
)

func TestOpenMissingLibraryReturnsError(t *testing.T) {
	path := filepath.Join(t.TempDir(), DefaultLibName())

	lib, err := Open(path)
	if err == nil {
		if closeErr := lib.Close(); closeErr != nil {
			t.Fatalf("close unexpected library: %v", closeErr)
		}
		t.Fatalf("Open(%q) succeeded, want error", path)
	}
	if lib != nil {
		t.Fatalf("Open(%q) returned non-nil library on error", path)
	}
}

func TestCloseNilAndZeroLibrary(t *testing.T) {
	var nilLibrary *Library
	if err := nilLibrary.Close(); err != nil {
		t.Fatalf("nil Close returned error: %v", err)
	}

	if err := (&Library{}).Close(); err != nil {
		t.Fatalf("zero-value Close returned error: %v", err)
	}
}
