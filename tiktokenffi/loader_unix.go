//go:build darwin || freebsd || linux || netbsd

package tiktokenffi

import "github.com/ebitengine/purego"

func openHandle(path string) (uintptr, error) {
	return purego.Dlopen(path, purego.RTLD_NOW)
}

func closeHandle(handle uintptr) error {
	return purego.Dlclose(handle)
}

func lookupSymbol(handle uintptr, name string) (uintptr, error) {
	return purego.Dlsym(handle, name)
}
