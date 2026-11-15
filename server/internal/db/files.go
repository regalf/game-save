package db

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"strings"
)

// File is the current state of one file inside a game namespace.
type File struct {
	Path      string `json:"path"`
	Size      int64  `json:"size"`
	SHA256    string `json:"sha256"`
	MTimeNS   int64  `json:"mtime_ns"`
	Version   int64  `json:"version"`
	Device    string `json:"device"`
	UpdatedAt string `json:"updated_at"`
}

// FileVersion is one entry in a file's history.
type FileVersion struct {
	Path      string `json:"path"`
	Size      int64  `json:"size"`
	SHA256    string `json:"sha256"`
	Version   int64  `json:"version"`
	Device    string `json:"device"`
	CreatedAt string `json:"created_at"`
}

// NormalizePath validates a relative file path coming from a client.
// Absolute paths, parent traversal and Windows drive letters are rejected.
func NormalizePath(p string) (string, error) {
	p = strings.TrimSpace(p)
	if p == "" {
		return "", fmt.Errorf("path must not be empty")
	}
	p = strings.ReplaceAll(p, "\\", "/")
	if strings.HasPrefix(p, "/") {
		return "", fmt.Errorf("path must be relative")
	}
	if len(p) >= 2 && p[1] == ':' {
		return "", fmt.Errorf("path must be relative")
	}
	parts := strings.Split(p, "/")
	out := make([]string, 0, len(parts))
	for _, seg := range parts {
		switch seg {
		case "", ".":
			continue
		case "..":
			return "", fmt.Errorf("path must not traverse upwards")
		}
		out = append(out, seg)
	}
	if len(out) == 0 {
		return "", fmt.Errorf("path must not be empty")
	}
	joined := strings.Join(out, "/")
	if len(joined) > 512 {
		return "", fmt.Errorf("path is too long")
	}
	return joined, nil
}

// ListFiles returns the current files of a game.
func (d *DB) ListFiles(ctx context.Context, gameID string) ([]File, error) {
	rows, err := d.sql.QueryContext(ctx,
		`SELECT path, size, sha256, mtime_ns, version, device, updated_at
		   FROM game_files WHERE game_id = ? ORDER BY path`, gameID)
	if err != nil {
		return nil, fmt.Errorf("list files: %w", err)
	}
	defer rows.Close()
	var out []File
	for rows.Next() {
		var f File
		if err := rows.Scan(&f.Path, &f.Size, &f.SHA256, &f.MTimeNS, &f.Version, &f.Device, &f.UpdatedAt); err != nil {
			return nil, fmt.Errorf("scan file: %w", err)
		}
		out = append(out, f)
	}
	return out, rows.Err()
}

// GetFile returns a single file, or ErrNotFound.
func (d *DB) GetFile(ctx context.Context, gameID, path string) (*File, error) {
	f := &File{}
	err := d.sql.QueryRowContext(ctx,
		`SELECT path, size, sha256, mtime_ns, version, device, updated_at
		   FROM game_files WHERE game_id = ? AND path = ?`, gameID, path).
		Scan(&f.Path, &f.Size, &f.SHA256, &f.MTimeNS, &f.Version, &f.Device, &f.UpdatedAt)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, fmt.Errorf("get file: %w", err)
	}
	return f, nil
}

// PutFile records a new version of a file.
//
// expectSHA implements the optimistic lock the client uses for conflict
// detection: when it is not empty and the stored file has a different
// digest, ErrConflict is returned and nothing is written.
func (d *DB) PutFile(ctx context.Context, gameID, path, sha string, size, mtimeNS int64, device, expectSHA string) (*File, error) {
	norm, err := NormalizePath(path)
	if err != nil {
		return nil, err
	}

	tx, err := d.sql.BeginTx(ctx, nil)
	if err != nil {
		return nil, fmt.Errorf("begin put file: %w", err)
	}
	defer tx.Rollback()

	var currentSHA string
	err = tx.QueryRowContext(ctx, `SELECT sha256 FROM game_files WHERE game_id = ? AND path = ?`, gameID, norm).Scan(&currentSHA)
	switch {
	case err == nil:
		if expectSHA != "" && !strings.EqualFold(currentSHA, expectSHA) {
			return nil, ErrConflict
		}
	case errors.Is(err, sql.ErrNoRows):
		if expectSHA != "" {
			// The client believed the file existed with this digest, but it
			// does not exist server side: report a conflict.
			return nil, ErrConflict
		}
	default:
		return nil, fmt.Errorf("read current file: %w", err)
	}

	ts := now()
	if _, err := tx.ExecContext(ctx,
		`UPDATE games SET version = version + 1, updated_at = ? WHERE id = ?`, ts, gameID); err != nil {
		return nil, fmt.Errorf("bump game version: %w", err)
	}
	var version int64
	if err := tx.QueryRowContext(ctx, `SELECT version FROM games WHERE id = ?`, gameID).Scan(&version); err != nil {
		return nil, fmt.Errorf("read game version: %w", err)
	}

	if _, err := tx.ExecContext(ctx,
		`INSERT INTO game_file_versions (game_id, path, size, sha256, version, device, created_at)
		 VALUES (?, ?, ?, ?, ?, ?, ?)`,
		gameID, norm, size, sha, version, device, ts); err != nil {
		return nil, fmt.Errorf("insert file version: %w", err)
	}

	if _, err := tx.ExecContext(ctx,
		`INSERT INTO game_files (game_id, path, size, sha256, mtime_ns, version, device, updated_at)
		 VALUES (?, ?, ?, ?, ?, ?, ?, ?)
		 ON CONFLICT(game_id, path) DO UPDATE SET
		   size = excluded.size, sha256 = excluded.sha256, mtime_ns = excluded.mtime_ns,
		   version = excluded.version, device = excluded.device, updated_at = excluded.updated_at`,
		gameID, norm, size, sha, mtimeNS, version, device, ts); err != nil {
		return nil, fmt.Errorf("upsert file: %w", err)
	}

	if _, err := tx.ExecContext(ctx,
		`INSERT INTO blobs (sha256, size, created_at) VALUES (?, ?, ?)
		 ON CONFLICT(sha256) DO NOTHING`, sha, size, ts); err != nil {
		return nil, fmt.Errorf("record blob: %w", err)
	}

	if err := tx.Commit(); err != nil {
		return nil, fmt.Errorf("commit put file: %w", err)
	}

	return &File{
		Path: norm, Size: size, SHA256: sha, MTimeNS: mtimeNS,
		Version: version, Device: device, UpdatedAt: ts,
	}, nil
}

// DeleteFile removes a file from a game and records the deletion in history.
//
// expectSHA implements the same optimistic lock as PutFile.
func (d *DB) DeleteFile(ctx context.Context, gameID, path, device, expectSHA string) error {
	norm, err := NormalizePath(path)
	if err != nil {
		return err
	}
	tx, err := d.sql.BeginTx(ctx, nil)
	if err != nil {
		return fmt.Errorf("begin delete file: %w", err)
	}
	defer tx.Rollback()

	var currentSHA string
	err = tx.QueryRowContext(ctx, `SELECT sha256 FROM game_files WHERE game_id = ? AND path = ?`, gameID, norm).Scan(&currentSHA)
	if errors.Is(err, sql.ErrNoRows) {
		return ErrNotFound
	}
	if err != nil {
		return fmt.Errorf("read current file: %w", err)
	}
	if expectSHA != "" && !strings.EqualFold(currentSHA, expectSHA) {
		return ErrConflict
	}

	ts := now()
	if _, err := tx.ExecContext(ctx,
		`UPDATE games SET version = version + 1, updated_at = ? WHERE id = ?`, ts, gameID); err != nil {
		return fmt.Errorf("bump game version: %w", err)
	}
	var version int64
	if err := tx.QueryRowContext(ctx, `SELECT version FROM games WHERE id = ?`, gameID).Scan(&version); err != nil {
		return fmt.Errorf("read game version: %w", err)
	}

	if _, err := tx.ExecContext(ctx,
		`INSERT INTO game_file_versions (game_id, path, size, sha256, version, device, created_at)
		 VALUES (?, ?, ?, ?, ?, ?, ?)`,
		gameID, norm, 0, currentSHA, version, device, ts); err != nil {
		return fmt.Errorf("insert deletion version: %w", err)
	}
	if _, err := tx.ExecContext(ctx,
		`DELETE FROM game_files WHERE game_id = ? AND path = ?`, gameID, norm); err != nil {
		return fmt.Errorf("delete file: %w", err)
	}
	return tx.Commit()
}

// FileVersions returns the history of a file, newest first.
func (d *DB) FileVersions(ctx context.Context, gameID, path string, limit int) ([]FileVersion, error) {
	norm, err := NormalizePath(path)
	if err != nil {
		return nil, err
	}
	if limit <= 0 || limit > 500 {
		limit = 100
	}
	rows, err := d.sql.QueryContext(ctx,
		`SELECT path, size, sha256, version, device, created_at FROM game_file_versions
		  WHERE game_id = ? AND path = ? ORDER BY version DESC LIMIT ?`, gameID, norm, limit)
	if err != nil {
		return nil, fmt.Errorf("file versions: %w", err)
	}
	defer rows.Close()
	var out []FileVersion
	for rows.Next() {
		var v FileVersion
		if err := rows.Scan(&v.Path, &v.Size, &v.SHA256, &v.Version, &v.Device, &v.CreatedAt); err != nil {
			return nil, fmt.Errorf("scan file version: %w", err)
		}
		out = append(out, v)
	}
	return out, rows.Err()
}

// ReferencedBlobs lists every blob digest still referenced by a game file.
func (d *DB) ReferencedBlobs(ctx context.Context) (map[string]bool, error) {
	rows, err := d.sql.QueryContext(ctx, `SELECT DISTINCT sha256 FROM game_files`)
	if err != nil {
		return nil, fmt.Errorf("referenced blobs: %w", err)
	}
	defer rows.Close()
	out := map[string]bool{}
	for rows.Next() {
		var s string
		if err := rows.Scan(&s); err != nil {
			return nil, fmt.Errorf("scan blob: %w", err)
		}
		out[s] = true
	}
	return out, rows.Err()
}

// AllBlobs lists every blob known to the database.
func (d *DB) AllBlobs(ctx context.Context) (map[string]int64, error) {
	rows, err := d.sql.QueryContext(ctx, `SELECT sha256, size FROM blobs`)
	if err != nil {
		return nil, fmt.Errorf("all blobs: %w", err)
	}
	defer rows.Close()
	out := map[string]int64{}
	for rows.Next() {
		var s string
		var size int64
		if err := rows.Scan(&s, &size); err != nil {
			return nil, fmt.Errorf("scan blob: %w", err)
		}
		out[s] = size
	}
	return out, rows.Err()
}

// ForgetBlob removes a blob row from the index.
func (d *DB) ForgetBlob(ctx context.Context, sha string) error {
	_, err := d.sql.ExecContext(ctx, `DELETE FROM blobs WHERE sha256 = ?`, sha)
	if err != nil {
		return fmt.Errorf("forget blob: %w", err)
	}
	return nil
}
