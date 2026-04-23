package tiktokenffi

import (
	"errors"
	"fmt"
	"runtime"
	"strings"
	"sync"
	"unsafe"

	"github.com/ebitengine/purego"
)

type SpecialMode uint32

const (
	SpecialModeDisallow SpecialMode = iota
	SpecialModeEncodeAsText
	SpecialModeAllowAll
	SpecialModeAllowList
)

type EncodeOptions struct {
	Mode           SpecialMode
	AllowedSpecial []string
}

type tokenBuffer struct {
	Data *uint32
	Len  uint64
}

type Library struct {
	mu     sync.Mutex
	handle uintptr

	version              func() *byte
	encodingNameForModel func(*byte, **byte, **byte) int32
	countWithEncoding    func(*byte, *byte, uint32, *byte, *uint64, **byte) int32
	countWithModel       func(*byte, *byte, uint32, *byte, *uint64, **byte) int32
	encodeWithEncoding   func(*byte, *byte, uint32, *byte, *tokenBuffer, **byte) int32
	encodeWithModel      func(*byte, *byte, uint32, *byte, *tokenBuffer, **byte) int32
	freeString           func(*byte)
	freeU32Buffer        func(*uint32, uint64)
}

func Open(path string) (*Library, error) {
	handle, err := openHandle(path)
	if err != nil {
		return nil, err
	}

	lib := &Library{handle: handle}
	if err := lib.registerSymbols(); err != nil {
		closeErr := closeHandle(handle)
		if closeErr != nil {
			return nil, fmt.Errorf("%w; also failed to close library: %v", err, closeErr)
		}
		return nil, err
	}

	return lib, nil
}

func (l *Library) Close() error {
	if l == nil {
		return nil
	}

	l.mu.Lock()
	defer l.mu.Unlock()

	if l.handle == 0 {
		return nil
	}
	if err := closeHandle(l.handle); err != nil {
		return err
	}

	l.reset()
	return nil
}

func (l *Library) Version() (string, error) {
	if err := l.lockOpen(); err != nil {
		return "", err
	}
	defer l.mu.Unlock()

	ptr := l.version()
	if ptr == nil {
		return "", errors.New("library returned nil version string")
	}
	defer l.freeString(ptr)

	return goString(ptr), nil
}

func (l *Library) EncodingNameForModel(model string) (string, error) {
	if err := l.lockOpen(); err != nil {
		return "", err
	}
	defer l.mu.Unlock()

	modelBytes, err := cStringBytes(model)
	if err != nil {
		return "", err
	}

	var out *byte
	var outErr *byte
	status := l.encodingNameForModel(&modelBytes[0], &out, &outErr)
	if status != 0 {
		return "", l.takeError(outErr)
	}
	if out == nil {
		return "", errors.New("library returned nil encoding name")
	}
	defer l.freeString(out)

	return goString(out), nil
}

func (l *Library) CountWithEncoding(encodingName, text string, options EncodeOptions) (uint64, error) {
	if err := l.lockOpen(); err != nil {
		return 0, err
	}
	defer l.mu.Unlock()

	encodingBytes, err := cStringBytes(encodingName)
	if err != nil {
		return 0, err
	}
	textBytes, err := cStringBytes(text)
	if err != nil {
		return 0, err
	}
	specialMode, specialBytes, err := encodeOptionsBytes(options)
	if err != nil {
		return 0, err
	}

	var count uint64
	var outErr *byte
	status := l.countWithEncoding(
		&encodingBytes[0],
		&textBytes[0],
		uint32(specialMode),
		&specialBytes[0],
		&count,
		&outErr,
	)
	if status != 0 {
		return 0, l.takeError(outErr)
	}
	return count, nil
}

func (l *Library) CountWithModel(modelName, text string, options EncodeOptions) (uint64, error) {
	if err := l.lockOpen(); err != nil {
		return 0, err
	}
	defer l.mu.Unlock()

	modelBytes, err := cStringBytes(modelName)
	if err != nil {
		return 0, err
	}
	textBytes, err := cStringBytes(text)
	if err != nil {
		return 0, err
	}
	specialMode, specialBytes, err := encodeOptionsBytes(options)
	if err != nil {
		return 0, err
	}

	var count uint64
	var outErr *byte
	status := l.countWithModel(
		&modelBytes[0],
		&textBytes[0],
		uint32(specialMode),
		&specialBytes[0],
		&count,
		&outErr,
	)
	if status != 0 {
		return 0, l.takeError(outErr)
	}
	return count, nil
}

func (l *Library) EncodeWithEncoding(encodingName, text string, options EncodeOptions) ([]uint32, error) {
	if err := l.lockOpen(); err != nil {
		return nil, err
	}
	defer l.mu.Unlock()

	encodingBytes, err := cStringBytes(encodingName)
	if err != nil {
		return nil, err
	}
	textBytes, err := cStringBytes(text)
	if err != nil {
		return nil, err
	}
	specialMode, specialBytes, err := encodeOptionsBytes(options)
	if err != nil {
		return nil, err
	}

	var buffer tokenBuffer
	var outErr *byte
	status := l.encodeWithEncoding(
		&encodingBytes[0],
		&textBytes[0],
		uint32(specialMode),
		&specialBytes[0],
		&buffer,
		&outErr,
	)
	if status != 0 {
		return nil, l.takeError(outErr)
	}
	defer l.freeU32Buffer(buffer.Data, buffer.Len)

	return copyTokens(buffer), nil
}

func (l *Library) EncodeWithModel(modelName, text string, options EncodeOptions) ([]uint32, error) {
	if err := l.lockOpen(); err != nil {
		return nil, err
	}
	defer l.mu.Unlock()

	modelBytes, err := cStringBytes(modelName)
	if err != nil {
		return nil, err
	}
	textBytes, err := cStringBytes(text)
	if err != nil {
		return nil, err
	}
	specialMode, specialBytes, err := encodeOptionsBytes(options)
	if err != nil {
		return nil, err
	}

	var buffer tokenBuffer
	var outErr *byte
	status := l.encodeWithModel(
		&modelBytes[0],
		&textBytes[0],
		uint32(specialMode),
		&specialBytes[0],
		&buffer,
		&outErr,
	)
	if status != 0 {
		return nil, l.takeError(outErr)
	}
	defer l.freeU32Buffer(buffer.Data, buffer.Len)

	return copyTokens(buffer), nil
}

func DefaultLibName() string {
	switch runtime.GOOS {
	case "darwin":
		return "libtiktoken_shim.dylib"
	case "windows":
		return "tiktoken_shim.dll"
	default:
		return "libtiktoken_shim.so"
	}
}

func encodeOptionsBytes(options EncodeOptions) (SpecialMode, []byte, error) {
	mode := options.Mode
	if mode == 0 && len(options.AllowedSpecial) > 0 {
		mode = SpecialModeAllowList
	}
	if mode == SpecialModeAllowList && len(options.AllowedSpecial) == 0 {
		return 0, nil, errors.New("AllowedSpecial must be non-empty when using SpecialModeAllowList")
	}

	list := ""
	if len(options.AllowedSpecial) > 0 {
		list = strings.Join(options.AllowedSpecial, "\n")
	}
	listBytes, err := cStringBytes(list)
	if err != nil {
		return 0, nil, err
	}

	return mode, listBytes, nil
}

func (l *Library) registerSymbols() error {
	registrations := []struct {
		name string
		fn   any
	}{
		{"tiktoken_version", &l.version},
		{"tiktoken_encoding_name_for_model", &l.encodingNameForModel},
		{"tiktoken_count_with_encoding", &l.countWithEncoding},
		{"tiktoken_count_with_model", &l.countWithModel},
		{"tiktoken_encode_with_encoding", &l.encodeWithEncoding},
		{"tiktoken_encode_with_model", &l.encodeWithModel},
		{"tiktoken_free_string", &l.freeString},
		{"tiktoken_free_u32_buffer", &l.freeU32Buffer},
	}

	for _, registration := range registrations {
		if err := registerSymbol(l.handle, registration.name, registration.fn); err != nil {
			l.reset()
			return err
		}
	}
	return nil
}

func registerSymbol(handle uintptr, name string, fn any) (err error) {
	symbol, err := lookupSymbol(handle, name)
	if err != nil {
		return fmt.Errorf("load symbol %q: %w", name, err)
	}

	defer func() {
		if recovered := recover(); recovered != nil {
			err = fmt.Errorf("register symbol %q: %v", name, recovered)
		}
	}()

	purego.RegisterFunc(fn, symbol)
	return nil
}

func (l *Library) lockOpen() error {
	if l == nil {
		return errors.New("library not initialized")
	}

	l.mu.Lock()
	if l.handle == 0 {
		l.mu.Unlock()
		return errors.New("library is closed")
	}
	return nil
}

func (l *Library) reset() {
	l.handle = 0
	l.version = nil
	l.encodingNameForModel = nil
	l.countWithEncoding = nil
	l.countWithModel = nil
	l.encodeWithEncoding = nil
	l.encodeWithModel = nil
	l.freeString = nil
	l.freeU32Buffer = nil
}

func cStringBytes(value string) ([]byte, error) {
	if strings.ContainsRune(value, '\x00') {
		return nil, fmt.Errorf("value contains NUL byte: %q", value)
	}
	bytes := make([]byte, len(value)+1)
	copy(bytes, value)
	return bytes, nil
}

func copyTokens(buffer tokenBuffer) []uint32 {
	if buffer.Len == 0 {
		return []uint32{}
	}
	if buffer.Data == nil {
		return nil
	}
	src := unsafe.Slice(buffer.Data, buffer.Len)
	dst := make([]uint32, len(src))
	copy(dst, src)
	return dst
}

func (l *Library) takeError(ptr *byte) error {
	if ptr == nil {
		return errors.New("unknown FFI error")
	}
	defer l.freeString(ptr)
	return errors.New(goString(ptr))
}

func goString(ptr *byte) string {
	if ptr == nil {
		return ""
	}
	length := 0
	for {
		b := *(*byte)(unsafe.Pointer(uintptr(unsafe.Pointer(ptr)) + uintptr(length)))
		if b == 0 {
			break
		}
		length++
	}
	return string(unsafe.Slice(ptr, length))
}
