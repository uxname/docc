package main

import (
	"archive/tar"
	"bytes"
	"compress/gzip"
	"crypto/sha256"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func releaseArchive(t *testing.T, content string) []byte {
	var buf bytes.Buffer
	gz := gzip.NewWriter(&buf)
	tw := tar.NewWriter(gz)
	_ = tw.WriteHeader(&tar.Header{Name: "docc_v2_linux_amd64/docc", Mode: 0o755, Size: int64(len(content)), Typeflag: tar.TypeReg})
	_, _ = tw.Write([]byte(content))
	_ = tw.Close()
	_ = gz.Close()
	return buf.Bytes()
}

func TestUpdate(t *testing.T) {
	arc := releaseArchive(t, "new-binary")
	sum := sha256.Sum256(arc)
	checks := fmt.Sprintf("%x  docc_v2_linux_amd64.tar.gz\n%x  docc_v2_darwin_arm64.tar.gz\n", sum, sum)
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case strings.HasSuffix(r.URL.Path, "/checksums.txt"):
			_, _ = w.Write([]byte(checks))
		case strings.HasSuffix(r.URL.Path, "/download/v2/docc_v2_linux_amd64.tar.gz"):
			_, _ = w.Write(arc)
		default:
			http.NotFound(w, r)
		}
	}))
	defer srv.Close()

	exe := filepath.Join(t.TempDir(), "docc")
	_ = os.WriteFile(exe, []byte("old-binary"), 0o755)
	var out bytes.Buffer

	if code, err := updateFrom(srv.URL, exe, "v1", "", true, "linux", "amd64", &out); code != 1 || err != nil || !strings.Contains(out.String(), "v2 is available") {
		t.Fatalf("--check: %d %v %q", code, err, out.String())
	}
	if code, err := updateFrom(srv.URL, exe, "v2", "", false, "linux", "amd64", &out); code != 0 || err != nil || !strings.Contains(out.String(), "up to date") {
		t.Fatalf("up to date: %d %v %q", code, err, out.String())
	}
	if _, err := updateFrom(srv.URL, exe, "v1", "", false, "linux", "riscv64", &out); err == nil {
		t.Fatal("a system without an archive must fail")
	}
	// a wrong checksum leaves the binary alone
	bad := strings.Replace(checks, fmt.Sprintf("%x", sum), strings.Repeat("0", 64), 1)
	checks, saved := bad, checks
	if _, err := updateFrom(srv.URL, exe, "v1", "", false, "linux", "amd64", &out); err == nil || !strings.Contains(err.Error(), "checksum") {
		t.Fatalf("checksum: %v", err)
	}
	if b, _ := os.ReadFile(exe); string(b) != "old-binary" {
		t.Fatal("binary changed despite a bad checksum")
	}
	checks = saved
	if code, err := updateFrom(srv.URL, exe, "v1", "", false, "linux", "amd64", &out); code != 0 || err != nil {
		t.Fatalf("update: %d %v", code, err)
	}
	if b, _ := os.ReadFile(exe); string(b) != "new-binary" {
		t.Fatalf("binary not replaced: %q", b)
	}
}
