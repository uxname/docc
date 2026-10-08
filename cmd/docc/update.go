package main

import (
	"archive/tar"
	"archive/zip"
	"bytes"
	"compress/gzip"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"io"
	"net/http"
	"os"
	"path"
	"path/filepath"
	"regexp"
	"runtime"
	"strings"
	"time"
)

// releasesURL is where the release archives and checksums.txt live; DOCC_BASE_URL overrides it
// (a mirror, or a test server). install.sh and install.ps1 read the same variable.
const releasesURL = "https://github.com/uxname/docc/releases"

const updateUsage = `Usage: docc update [--check] [--version vX.Y.Z]

Downloads the release archive for this system, checks it against checksums.txt of the release
and replaces the running docc. Without --version it takes the latest release.
--check only tells whether a newer release exists (exit code 1: there is one).

Examples:
  docc update
  docc update --check
  docc update --version v0.2.0
`

func cmdUpdate(args []string, stdout io.Writer) (int, error) {
	check, want := false, ""
	for i := 0; i < len(args); i++ {
		switch args[i] {
		case "--check":
			check = true
		case "--version":
			if i++; i == len(args) {
				return 2, usageErr("--version needs a value, for example v0.2.0")
			}
			want = args[i]
		case "--help", "-h":
			return 0, &helpError{updateUsage}
		default:
			return 2, usageErr("unexpected argument %q for docc update", args[i])
		}
	}
	cur := buildVersion()
	if cur == "dev" {
		return 2, usageErr("this is a development build with no release version: build from source again or install a release")
	}
	exe, err := os.Executable()
	if err != nil {
		return 2, err
	}
	if exe, err = filepath.EvalSymlinks(exe); err != nil {
		return 2, err
	}
	base := os.Getenv("DOCC_BASE_URL")
	if base == "" {
		base = releasesURL
	}
	return updateFrom(base, exe, cur, want, check, runtime.GOOS, runtime.GOARCH, stdout)
}

// archiveRe matches a line of checksums.txt: "<sha256>  docc_<version>_<os>_<arch>.<tar.gz|zip>".
var archiveRe = regexp.MustCompile(`^([0-9a-f]{64})\s+\*?(docc_(\S+)_([a-z0-9]+)_([a-z0-9]+)\.(?:tar\.gz|zip))$`)

func updateFrom(base, exe, cur, want string, check bool, goos, goarch string, stdout io.Writer) (int, error) {
	listURL := base + "/latest/download/checksums.txt"
	if want != "" {
		listURL = base + "/download/" + want + "/checksums.txt"
	}
	list, err := fetch(listURL)
	if err != nil {
		return 2, fmt.Errorf("cannot read the release list: %w", err)
	}
	var sum, name, latest string
	for _, line := range strings.Split(string(list), "\n") {
		if m := archiveRe.FindStringSubmatch(strings.TrimSpace(line)); m != nil && m[4] == goos && m[5] == goarch {
			sum, name, latest = m[1], m[2], m[3]
		}
	}
	if name == "" {
		return 2, fmt.Errorf("the release has no archive for %s/%s", goos, goarch)
	}
	if latest == cur {
		say(stdout, "docc %s is up to date\n", cur)
		return 0, nil
	}
	if check {
		say(stdout, "docc %s is available (installed: %s); run docc update\n", latest, cur)
		return 1, nil
	}
	data, err := fetch(base + "/download/" + latest + "/" + name)
	if err != nil {
		return 2, fmt.Errorf("download failed: %w", err)
	}
	if got := sha256.Sum256(data); hex.EncodeToString(got[:]) != sum {
		return 2, fmt.Errorf("checksum of %s does not match checksums.txt: nothing was changed", name)
	}
	bin, err := extractBinary(data, name)
	if err != nil {
		return 2, err
	}
	if err := replaceExe(exe, bin); err != nil {
		return 2, fmt.Errorf("cannot replace %s: %w (is the folder writable? try the install script)", exe, err)
	}
	say(stdout, "updated docc %s -> %s\n", cur, latest)
	return 0, nil
}

func fetch(url string) ([]byte, error) {
	resp, err := (&http.Client{Timeout: 2 * time.Minute}).Get(url)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("%s: %s", url, resp.Status)
	}
	return io.ReadAll(io.LimitReader(resp.Body, 200<<20))
}

// extractBinary returns docc (docc.exe) from a release archive.
func extractBinary(data []byte, name string) ([]byte, error) {
	isBin := func(p string) bool { b := path.Base(p); return b == "docc" || b == "docc.exe" }
	if strings.HasSuffix(name, ".zip") {
		zr, err := zip.NewReader(bytes.NewReader(data), int64(len(data)))
		if err != nil {
			return nil, err
		}
		for _, f := range zr.File {
			if isBin(f.Name) {
				rc, err := f.Open()
				if err != nil {
					return nil, err
				}
				defer rc.Close()
				return io.ReadAll(rc)
			}
		}
	} else {
		gz, err := gzip.NewReader(bytes.NewReader(data))
		if err != nil {
			return nil, err
		}
		tr := tar.NewReader(gz)
		for {
			h, err := tr.Next()
			if err != nil {
				break
			}
			if h.Typeflag == tar.TypeReg && isBin(h.Name) {
				return io.ReadAll(tr)
			}
		}
	}
	return nil, fmt.Errorf("%s has no docc binary", name)
}

// replaceExe swaps the file in place. A running exe cannot be overwritten on Windows but can be renamed.
func replaceExe(exe string, bin []byte) error {
	tmp := exe + ".new"
	if err := os.WriteFile(tmp, bin, 0o755); err != nil {
		return err
	}
	if runtime.GOOS == "windows" {
		_ = os.Remove(exe + ".old")
		if err := os.Rename(exe, exe+".old"); err != nil {
			_ = os.Remove(tmp)
			return err
		}
	}
	if err := os.Rename(tmp, exe); err != nil {
		_ = os.Remove(tmp)
		return err
	}
	return nil
}
