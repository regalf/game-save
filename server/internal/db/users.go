package db

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"strings"
)

// ErrNotFound is returned when a row does not exist.
var ErrNotFound = errors.New("not found")

// ErrConflict is returned when an optimistic lock check fails.
var ErrConflict = errors.New("conflict")

// User is an account owning games and API keys.
type User struct {
	ID          int64
	Username    string
	DisplayName string
	Disabled    bool
	CreatedAt   string
}

// APIKey is the metadata of an issued key. The secret itself is never stored.
type APIKey struct {
	ID         int64
	UserID     int64
	Label      string
	Prefix     string
	CreatedAt  string
	LastUsedAt string
	RevokedAt  string
}

// CreateUser inserts a new user and returns it.
func (d *DB) CreateUser(ctx context.Context, username, displayName string) (*User, error) {
	username = strings.TrimSpace(username)
	if username == "" {
		return nil, fmt.Errorf("username must not be empty")
	}
	if displayName == "" {
		displayName = username
	}
	res, err := d.sql.ExecContext(ctx,
		`INSERT INTO users (username, display_name, disabled, created_at) VALUES (?, ?, 0, ?)`,
		username, displayName, now())
	if err != nil {
		if strings.Contains(err.Error(), "UNIQUE") {
			return nil, fmt.Errorf("user %q already exists", username)
		}
		return nil, fmt.Errorf("insert user: %w", err)
	}
	id, err := res.LastInsertId()
	if err != nil {
		return nil, fmt.Errorf("insert user id: %w", err)
	}
	return &User{ID: id, Username: username, DisplayName: displayName, CreatedAt: now()}, nil
}

// UserByUsername looks a user up by name.
func (d *DB) UserByUsername(ctx context.Context, username string) (*User, error) {
	u := &User{}
	var disabled int
	err := d.sql.QueryRowContext(ctx,
		`SELECT id, username, display_name, disabled, created_at FROM users WHERE username = ?`, username).
		Scan(&u.ID, &u.Username, &u.DisplayName, &disabled, &u.CreatedAt)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, fmt.Errorf("select user: %w", err)
	}
	u.Disabled = disabled != 0
	return u, nil
}

// UserByID looks a user up by identifier.
func (d *DB) UserByID(ctx context.Context, id int64) (*User, error) {
	u := &User{}
	var disabled int
	err := d.sql.QueryRowContext(ctx,
		`SELECT id, username, display_name, disabled, created_at FROM users WHERE id = ?`, id).
		Scan(&u.ID, &u.Username, &u.DisplayName, &disabled, &u.CreatedAt)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, fmt.Errorf("select user: %w", err)
	}
	u.Disabled = disabled != 0
	return u, nil
}

// ListUsers returns every account, ordered by name.
func (d *DB) ListUsers(ctx context.Context) ([]User, error) {
	rows, err := d.sql.QueryContext(ctx,
		`SELECT id, username, display_name, disabled, created_at FROM users ORDER BY username`)
	if err != nil {
		return nil, fmt.Errorf("list users: %w", err)
	}
	defer rows.Close()
	var out []User
	for rows.Next() {
		var u User
		var disabled int
		if err := rows.Scan(&u.ID, &u.Username, &u.DisplayName, &disabled, &u.CreatedAt); err != nil {
			return nil, fmt.Errorf("scan user: %w", err)
		}
		u.Disabled = disabled != 0
		out = append(out, u)
	}
	return out, rows.Err()
}

// SetUserDisabled enables or disables an account.
func (d *DB) SetUserDisabled(ctx context.Context, userID int64, disabled bool) error {
	v := 0
	if disabled {
		v = 1
	}
	res, err := d.sql.ExecContext(ctx, `UPDATE users SET disabled = ? WHERE id = ?`, v, userID)
	if err != nil {
		return fmt.Errorf("update user: %w", err)
	}
	if n, _ := res.RowsAffected(); n == 0 {
		return ErrNotFound
	}
	return nil
}

// InsertAPIKey stores the hash of a newly issued key.
func (d *DB) InsertAPIKey(ctx context.Context, userID int64, label, prefix, hash string) (*APIKey, error) {
	res, err := d.sql.ExecContext(ctx,
		`INSERT INTO api_keys (user_id, label, key_prefix, key_hash, created_at) VALUES (?, ?, ?, ?, ?)`,
		userID, label, prefix, hash, now())
	if err != nil {
		return nil, fmt.Errorf("insert api key: %w", err)
	}
	id, err := res.LastInsertId()
	if err != nil {
		return nil, fmt.Errorf("insert api key id: %w", err)
	}
	return &APIKey{ID: id, UserID: userID, Label: label, Prefix: prefix, CreatedAt: now()}, nil
}

// APIKeyByHash resolves a presented key to its record. Revoked keys and keys
// belonging to disabled users are reported as not found.
func (d *DB) APIKeyByHash(ctx context.Context, hash string) (*APIKey, *User, error) {
	k := &APIKey{}
	var revoked sql.NullString
	var lastUsed sql.NullString
	err := d.sql.QueryRowContext(ctx,
		`SELECT id, user_id, label, key_prefix, created_at, last_used_at, revoked_at
		   FROM api_keys WHERE key_hash = ?`, hash).
		Scan(&k.ID, &k.UserID, &k.Label, &k.Prefix, &k.CreatedAt, &lastUsed, &revoked)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, nil, ErrNotFound
	}
	if err != nil {
		return nil, nil, fmt.Errorf("select api key: %w", err)
	}
	if revoked.Valid {
		return nil, nil, ErrNotFound
	}
	if lastUsed.Valid {
		k.LastUsedAt = lastUsed.String
	}
	if revoked.Valid {
		k.RevokedAt = revoked.String
	}

	u, err := d.UserByID(ctx, k.UserID)
	if err != nil {
		return nil, nil, err
	}
	if u.Disabled {
		return nil, nil, ErrNotFound
	}
	return k, u, nil
}

// TouchAPIKey records the last use of a key.
func (d *DB) TouchAPIKey(ctx context.Context, keyID int64) {
	_, _ = d.sql.ExecContext(ctx, `UPDATE api_keys SET last_used_at = ? WHERE id = ?`, now(), keyID)
}

// ListAPIKeys returns the keys of a user.
func (d *DB) ListAPIKeys(ctx context.Context, userID int64) ([]APIKey, error) {
	rows, err := d.sql.QueryContext(ctx,
		`SELECT id, user_id, label, key_prefix, created_at, last_used_at, revoked_at
		   FROM api_keys WHERE user_id = ? ORDER BY id`, userID)
	if err != nil {
		return nil, fmt.Errorf("list api keys: %w", err)
	}
	defer rows.Close()
	var out []APIKey
	for rows.Next() {
		var k APIKey
		var lastUsed, revoked sql.NullString
		if err := rows.Scan(&k.ID, &k.UserID, &k.Label, &k.Prefix, &k.CreatedAt, &lastUsed, &revoked); err != nil {
			return nil, fmt.Errorf("scan api key: %w", err)
		}
		if lastUsed.Valid {
			k.LastUsedAt = lastUsed.String
		}
		if revoked.Valid {
			k.RevokedAt = revoked.String
		}
		out = append(out, k)
	}
	return out, rows.Err()
}

// RevokeAPIKey marks a key as revoked. Revoked keys stop working immediately.
func (d *DB) RevokeAPIKey(ctx context.Context, userID int64, keyID int64) error {
	res, err := d.sql.ExecContext(ctx,
		`UPDATE api_keys SET revoked_at = ? WHERE id = ? AND user_id = ? AND revoked_at IS NULL`,
		now(), keyID, userID)
	if err != nil {
		return fmt.Errorf("revoke api key: %w", err)
	}
	if n, _ := res.RowsAffected(); n == 0 {
		return ErrNotFound
	}
	return nil
}
