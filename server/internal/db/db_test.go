package db

import (
	"context"
	"errors"
	"path/filepath"
	"testing"
)

func newTestDB(t *testing.T) *DB {
	t.Helper()
	d, err := Open(filepath.Join(t.TempDir(), "test.db"))
	if err != nil {
		t.Fatalf("Open: %v", err)
	}
	t.Cleanup(func() { d.Close() })
	if err := d.Migrate(context.Background()); err != nil {
		t.Fatalf("Migrate: %v", err)
	}
	return d
}

func TestMigrateIsIdempotent(t *testing.T) {
	d := newTestDB(t)
	ctx := context.Background()
	if err := d.Migrate(ctx); err != nil {
		t.Fatalf("second Migrate: %v", err)
	}
	var n int
	if err := d.SQL().QueryRowContext(ctx, `SELECT COUNT(*) FROM schema_migrations`).Scan(&n); err != nil {
		t.Fatalf("count migrations: %v", err)
	}
	if n != 1 {
		t.Fatalf("expected 1 applied migration, got %d", n)
	}
}

func TestUserAndKeyLifecycle(t *testing.T) {
	d := newTestDB(t)
	ctx := context.Background()

	u, err := d.CreateUser(ctx, "alice", "")
	if err != nil {
		t.Fatalf("CreateUser: %v", err)
	}
	if u.DisplayName != "alice" {
		t.Fatalf("display name should default to the username, got %q", u.DisplayName)
	}
	if _, err := d.CreateUser(ctx, "alice", ""); err == nil {
		t.Fatal("duplicate user must be rejected")
	}

	if _, err := d.InsertAPIKey(ctx, u.ID, "laptop", "gsk_abcdefgh", "hash1"); err != nil {
		t.Fatalf("InsertAPIKey: %v", err)
	}
	if _, err := d.InsertAPIKey(ctx, u.ID, "desktop", "gsk_ijklmnop", "hash2"); err != nil {
		t.Fatalf("InsertAPIKey: %v", err)
	}

	keys, err := d.ListAPIKeys(ctx, u.ID)
	if err != nil {
		t.Fatalf("ListAPIKeys: %v", err)
	}
	if len(keys) != 2 {
		t.Fatalf("expected 2 keys, got %d", len(keys))
	}

	key, user, err := d.APIKeyByHash(ctx, "hash1")
	if err != nil {
		t.Fatalf("APIKeyByHash: %v", err)
	}
	if user.Username != "alice" {
		t.Fatalf("wrong user resolved: %q", user.Username)
	}
	if key.Label != "laptop" {
		t.Fatalf("wrong label: %q", key.Label)
	}

	if err := d.RevokeAPIKey(ctx, u.ID, key.ID); err != nil {
		t.Fatalf("RevokeAPIKey: %v", err)
	}
	if _, _, err := d.APIKeyByHash(ctx, "hash1"); !errors.Is(err, ErrNotFound) {
		t.Fatalf("a revoked key must not resolve, got %v", err)
	}
	if err := d.RevokeAPIKey(ctx, u.ID, key.ID); !errors.Is(err, ErrNotFound) {
		t.Fatalf("revoking twice must report not found, got %v", err)
	}
}

func TestDisabledUserCannotAuthenticate(t *testing.T) {
	d := newTestDB(t)
	ctx := context.Background()

	u, _ := d.CreateUser(ctx, "bob", "")
	if _, err := d.InsertAPIKey(ctx, u.ID, "k", "gsk_abcdefgh", "hash1"); err != nil {
		t.Fatalf("InsertAPIKey: %v", err)
	}
	if err := d.SetUserDisabled(ctx, u.ID, true); err != nil {
		t.Fatalf("SetUserDisabled: %v", err)
	}
	if _, _, err := d.APIKeyByHash(ctx, "hash1"); !errors.Is(err, ErrNotFound) {
		t.Fatalf("a disabled user must not authenticate, got %v", err)
	}
}

func TestGameIsolationBetweenUsers(t *testing.T) {
	d := newTestDB(t)
	ctx := context.Background()

	alice, _ := d.CreateUser(ctx, "alice", "")
	bob, _ := d.CreateUser(ctx, "bob", "")

	g, err := d.CreateGame(ctx, alice.ID, "Hollow Knight", "")
	if err != nil {
		t.Fatalf("CreateGame: %v", err)
	}
	if g.Slug != "hollow-knight" {
		t.Fatalf("unexpected slug %q", g.Slug)
	}

	if _, err := d.GameByID(ctx, bob.ID, g.ID); !errors.Is(err, ErrNotFound) {
		t.Fatal("a user must not see another user's game")
	}
	if _, err := d.GameBySlug(ctx, bob.ID, "hollow-knight"); !errors.Is(err, ErrNotFound) {
		t.Fatal("a user must not resolve another user's slug")
	}

	if _, err := d.CreateGame(ctx, bob.ID, "Hollow Knight", ""); err != nil {
		t.Fatalf("the same slug must be usable by another user: %v", err)
	}
}

func TestPutFileOptimisticLock(t *testing.T) {
	d := newTestDB(t)
	ctx := context.Background()

	u, _ := d.CreateUser(ctx, "alice", "")
	g, _ := d.CreateGame(ctx, u.ID, "Game", "")

	f, err := d.PutFile(ctx, g.ID, "saves/slot1.dat", "aaa", 10, 111, "pc", "")
	if err != nil {
		t.Fatalf("PutFile: %v", err)
	}
	if f.Path != "saves/slot1.dat" {
		t.Fatalf("unexpected path %q", f.Path)
	}
	if f.Version != 2 {
		t.Fatalf("unexpected version %d", f.Version)
	}

	// The client believes the server holds "aaa": the upload is accepted.
	if _, err := d.PutFile(ctx, g.ID, "saves/slot1.dat", "bbb", 20, 222, "pc", "aaa"); err != nil {
		t.Fatalf("PutFile with matching expectation: %v", err)
	}

	// The client still believes the server holds "aaa": the upload is rejected.
	_, err = d.PutFile(ctx, g.ID, "saves/slot1.dat", "ccc", 30, 333, "pc", "aaa")
	if !errors.Is(err, ErrConflict) {
		t.Fatalf("expected a conflict, got %v", err)
	}

	// The rejected upload must not have changed anything.
	cur, err := d.GetFile(ctx, g.ID, "saves/slot1.dat")
	if err != nil {
		t.Fatalf("GetFile: %v", err)
	}
	if cur.SHA256 != "bbb" {
		t.Fatalf("a rejected upload changed the file: %q", cur.SHA256)
	}

	versions, err := d.FileVersions(ctx, g.ID, "saves/slot1.dat", 10)
	if err != nil {
		t.Fatalf("FileVersions: %v", err)
	}
	if len(versions) != 2 {
		t.Fatalf("expected 2 versions in history, got %d", len(versions))
	}
}

func TestDeleteFileRecordsHistory(t *testing.T) {
	d := newTestDB(t)
	ctx := context.Background()

	u, _ := d.CreateUser(ctx, "alice", "")
	g, _ := d.CreateGame(ctx, u.ID, "Game", "")
	if _, err := d.PutFile(ctx, g.ID, "saves/a.dat", "aaa", 10, 0, "pc", ""); err != nil {
		t.Fatalf("PutFile: %v", err)
	}
	if err := d.DeleteFile(ctx, g.ID, "saves/a.dat", "pc", ""); err != nil {
		t.Fatalf("DeleteFile: %v", err)
	}
	if _, err := d.GetFile(ctx, g.ID, "saves/a.dat"); !errors.Is(err, ErrNotFound) {
		t.Fatal("the file must be gone")
	}
	versions, err := d.FileVersions(ctx, g.ID, "saves/a.dat", 10)
	if err != nil {
		t.Fatalf("FileVersions: %v", err)
	}
	if len(versions) != 2 {
		t.Fatalf("the deletion must be recorded in history, got %d entries", len(versions))
	}
}

func TestNormalizePath(t *testing.T) {
	cases := []struct {
		in      string
		want    string
		wantErr bool
	}{
		{"saves/slot1.dat", "saves/slot1.dat", false},
		{`saves\slot1.dat`, "saves/slot1.dat", false},
		{"./saves//slot1.dat", "saves/slot1.dat", false},
		{"", "", true},
		{"/etc/passwd", "", true},
		{"../escape", "", true},
		{"saves/../../escape", "", true},
		{`C:\Windows\System32`, "", true},
	}
	for _, tc := range cases {
		got, err := NormalizePath(tc.in)
		if tc.wantErr {
			if err == nil {
				t.Errorf("NormalizePath(%q) must fail", tc.in)
			}
			continue
		}
		if err != nil {
			t.Errorf("NormalizePath(%q): %v", tc.in, err)
			continue
		}
		if got != tc.want {
			t.Errorf("NormalizePath(%q) = %q, want %q", tc.in, got, tc.want)
		}
	}
}
