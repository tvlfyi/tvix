package importer

import (
	"io"
)

// CountingWriter implements io.Writer.
var _ io.Writer = &CountingWriter{}

type CountingWriter struct {
	bytesWritten uint64
}

func (cw *CountingWriter) Write(p []byte) (n int, err error) {
	cw.bytesWritten += uint64(len(p))
	return len(p), nil
}

func (cw *CountingWriter) BytesWritten() uint64 {
	return cw.bytesWritten
}
