//go:build windows

package tiktokenffi

import "golang.org/x/sys/windows"

func openHandle(path string) (uintptr, error) {
	handle, err := windows.LoadLibrary(path)
	return uintptr(handle), err
}

func closeHandle(handle uintptr) error {
	return windows.FreeLibrary(windows.Handle(handle))
}

func lookupSymbol(handle uintptr, name string) (uintptr, error) {
	return windows.GetProcAddress(windows.Handle(handle), name)
}
