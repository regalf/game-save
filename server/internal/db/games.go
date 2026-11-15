package db

import (
	"context"
	"crypto/rand"
	"database/sql"
	"encoding/hex"
	"errors"
	"fmt"
	"regexp"
	"strings"
)

// Game is one sync namespace: the cloud side of a single game's save folder.
type Game struct {
	ID        string `json:"id"`
	UserID    int64  `json:"user_id"`
	Name      string `json:"name"`
	Slug      string `json:"slug"`
	Version   int64  `json:"version"`
	CreatedAt string `json:"created_at"`
	UpdatedAt string `json:"updated_at"`
}

var slugRe = regexp.MustCompile(`[^a-z0-9]+`)

// Slugify converts a display name into a filesystem and URL friendly slug.
func Slugify(name string) string {
	s := strings.ToLower(strings.TrimSpace(name))
	s = slugRe.ReplaceAllString(s, "-")
	s = strings.Trim(s, "-")
	if s == "" {
		s = "game"
	}
	if len(s) > 48 {
		s = strings.Trim(s[:48], "-")
	}
	return s
}

// randomID returns a short opaque identifier.
func randomID() (string, error) {
	b := make([]byte, 10)
	if _, err := rand.Read(b); err != nil {
		return "", fmt.Errorf("random id: %w", err)
	}
	return hex.EncodeToString(b), nil
}

// CreateGame registers a new game namespace for a user.
func (d *DB) CreateGame(ctx context.Context, userID int64, name, slug string) (*Game, error) {
	name = strings.TrimSpace(name)
	if name == "" {
		return nil, fmt.Errorf("game name must not be empty")
	}
	if slug == "" {
		slug = Slugify(name)
	}
	id, err := randomID()
	if err != nil {
		return nil, err
	}
	ts := now()
	res, err := d.sql.ExecContext(ctx,
		`INSERT INTO games (id, user_id, name, slug, version, created_at, updated_at) VALUES (?, ?, ?, ?, 1, ?, ?)`,
		id, userID, name, slug, ts, ts)
	if err != nil {
		if strings.Contains(err.Error(), "UNIQUE") {
			return nil, fmt.Errorf("game %q already exists for this user", slug)
		}
		return nil, fmt.Errorf("insert game: %w", err)
	}
	if n, _ := res.RowsAffected(); n == 0 {
		return nil, fmt.Errorf("insert game: no row affected")
	}
	return &Game{ID: id, UserID: userID, Name: name, Slug: slug, Version: 1, CreatedAt: ts, UpdatedAt: ts}, nil
}

// GameByID loads a game, enforcing ownership.
func (d *DB) GameByID(ctx context.Context, userID int64, gameID string) (*Game, error) {
	g := &Game{}
	err := d.sql.QueryRowContext(ctx,
		`SELECT id, user_id, name, slug, version, created_at, updated_at FROM games WHERE id = ? AND user_id = ?`,
		gameID, userID).
		Scan(&g.ID, &g.UserID, &g.Name, &g.Slug, &g.Version, &g.CreatedAt, &g.UpdatedAt)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, fmt.Errorf("select game: %w", err)
	}
	return g, nil
}

// GameBySlug loads a game by its slug, enforcing ownership.
func (d *DB) GameBySlug(ctx context.Context, userID int64, slug string) (*Game, error) {
	g := &Game{}
	err := d.sql.QueryRowContext(ctx,
		`SELECT id, user_id, name, slug, version, created_at, updated_at FROM games WHERE slug = ? AND user_id = ?`,
		slug, userID).
		Scan(&g.ID, &g.UserID, &g.Name, &g.Slug, &g.Version, &g.CreatedAt, &g.UpdatedAt)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, fmt.Errorf("select game: %w", err)
	}
	return g, nil
}

// ListGames returns every game of a user.
func (d *DB) ListGames(ctx context.Context, userID int64) ([]Game, error) {
	rows, err := d.sql.QueryContext(ctx,
		`SELECT id, user_id, name, slug, version, created_at, updated_at FROM games WHERE user_id = ? ORDER BY name`,
		userID)
	if err != nil {
		return nil, fmt.Errorf("list games: %w", err)
	}
	defer rows.Close()
	var out []Game
	for rows.Next() {
		var g Game
		if err := rows.Scan(&g.ID, &g.UserID, &g.Name, &g.Slug, &g.Version, &g.CreatedAt, &g.UpdatedAt); err != nil {
			return nil, fmt.Errorf("scan game: %w", err)
		}
		out = append(out, g)
	}
	return out, rows.Err()
}

// RenameGame changes the display name of a game.
func (d *DB) RenameGame(ctx context.Context, userID int64, gameID, name string) error {
	name = strings.TrimSpace(name)
	if name == "" {
		return fmt.Errorf("game name must not be empty")
	}
	res, err := d.sql.ExecContext(ctx,
		`UPDATE games SET name = ?, updated_at = ? WHERE id = ? AND user_id = ?`, name, now(), gameID, userID)
	if err != nil {
		return fmt.Errorf("rename game: %w", err)
	}
	if n, _ := res.RowsAffected(); n == 0 {
		return ErrNotFound
	}
	return nil
}

// DeleteGame removes a game and, by cascade, its files and history.
func (d *DB) DeleteGame(ctx context.Context, userID int64, gameID string) error {
	res, err := d.sql.ExecContext(ctx, `DELETE FROM games WHERE id = ? AND user_id = ?`, gameID, userID)
	if err != nil {
		return fmt.Errorf("delete game: %w", err)
	}
	if n, _ := res.RowsAffected(); n == 0 {
		return ErrNotFound
	}
	return nil
}
