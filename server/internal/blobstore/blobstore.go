// Package blobstore implements the content addressed store that keeps the
// actual file contents on disk.
//
// Layout: <root>/ab/cd/<sha256>, where "ab" and "cd" are the first four hex
// characters of the digest. Identical content is stored once, no matter how
// many games or users reference it.
package blobstore

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
)

// Store is a content addressed blob store rooted at a directory.
type Store struct {
	root string
}

// New creates the store and its directory tree.
func New(root string) (*Store, error) {
	if err := os.MkdirAll(root, 0o755); err != nil {
		return nil, fmt.Errorf("create blob dir: %w", err)
	}
	return &Store{root: root}, nil
}

// Root returns the directory holding the store.
func (s *Store) Root() string { return s.root }

// Path returns the on disk location of a blob.
func (s *Store) Path(digest string) string {
	d := strings.ToLower(digest)
	if len(d) < 4 {
		return filepath.Join(s.root, d)
	}
	return filepath.Join(s.root, d[0:2], d[2:4], d)
}

// Has reports whether a blob is present.
func (s *Store) Has(digest string) bool {
	_, err := os.Stat(s.Path(digest))
	return err == nil
}

// Size returns the size of a stored blob.
func (s *Store) Size(digest string) (int64, error) {
	fi, err := os.Stat(s.Path(digest))
	if err != nil {
		return 0, fmt.Errorf("stat blob: %w", err)
	}
	return fi.Size(), nil
}

// Open returns a reader for a stored blob.
func (s *Store) Open(digest string) (*os.File, error) {
	f, err := os.Open(s.Path(digest))
	if err != nil {
		return nil, fmt.Errorf("open blob: %w", err)
	}
	return f, nil
}

// Put streams the content of r into the store and returns its digest and size.
//
// The content is first written to a temporary file while the digest is
// computed, then moved into place. If a blob with the same digest already
// exists the temporary file is discarded.
func (s *Store) Put(r io.Reader) (string, int64, error) {
	tmp, err := os.CreateTemp(s.root, ".upload-*")
	if err != nil {
		return "", 0, fmt.Errorf("create temp file: %w", err)
	}
	tmpName := tmp.Name()
	defer func() {
		tmp.Close()
		os.Remove(tmpName)
	}()

	hasher := sha256.New()
	size, err := io.Copy(io.MultiWriter(tmp, hasher), r)
	if err != nil {
		return "", 0, fmt.Errorf("write blob: %w", err)
	}
	if err := tmp.Sync(); err != nil {
		return "", 0, fmt.Errorf("sync blob: %w", err)
	}
	if err := tmp.Close(); err != nil {
		return "", 0, fmt.Errorf("close blob: %w", err)
	}

	digest := hex.EncodeToString(hasher.Sum(nil))
	dest := s.Path(digest)
	if _, err := os.Stat(dest); err == nil {
		return digest, size, nil // already stored
	}
	if err := os.MkdirAll(filepath.Dir(dest), 0o755); err != nil {
		return "", 0, fmt.Errorf("create blob dir: %w", err)
	}
	if err := os.Rename(tmpName, dest); err != nil {
		return "", 0, fmt.Errorf("move blob: %w", err)
	}
	return digest, size, nil
}

// Remove deletes a blob from the store.
func (s *Store) Remove(digest string) error {
	if err := os.Remove(s.Path(digest)); err != nil && !os.IsNotExist(err) {
		return fmt.Errorf("remove blob: %w", err)
	}
	return nil
}

// List returns the digests of every blob present in the store.
func (s *Store) List() ([]string, error) {
	var out []string
	err := filepath.WalkDir(s.root, func(path string, d os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if d.IsDir() || strings.HasPrefix(d.Name(), ".upload-") {
			return nil
		}
		name := d.Name()
		if len(name) != 64 {
			return nil
		}
		if _, err := hex.DecodeString(name); err != nil {
			return nil
		}
		out = append(out, strings.ToLower(name))
		return nil
	})
	if err != nil {
		return nil, fmt.Errorf("walk blob dir: %w", err)
	}
	return out, nil
}
