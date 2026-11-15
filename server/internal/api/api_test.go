package api

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"net/url"
		"testing"

	"gamesave/internal/auth"
	"gamesave/internal/blobstore"
	"gamesave/internal/config"
	"gamesave/internal/db"
)

type testEnv struct {
	t      *testing.T
	server *httptest.Server
	db     *db.DB
	blobs  *blobstore.Store
}

func newTestEnv(t *testing.T) *testEnv {
	t.Helper()
	dir := t.TempDir()

	cfg := config.Default()
	cfg.Server.DataDir = dir
	cfg.Limits.MaxFileSize = 1 << 20

	database, err := db.Open(cfg.DBPath())
	if err != nil {
		t.Fatalf("open db: %v", err)
	}
	t.Cleanup(func() { database.Close() })
	if err := database.Migrate(context.Background()); err != nil {
		t.Fatalf("migrate: %v", err)
	}

	blobs, err := blobstore.New(cfg.BlobDir())
	if err != nil {
		t.Fatalf("blobstore: %v", err)
	}

	srv := New(cfg, database, blobs, testLogger())
	ts := httptest.NewServer(srv.Handler())
	t.Cleanup(ts.Close)

	return &testEnv{t: t, server: ts, db: database, blobs: blobs}
}

// testLogger returns a logger that discards everything.
func testLogger() *slog.Logger {
	return slog.New(slog.NewTextHandler(io.Discard, &slog.HandlerOptions{Level: slog.LevelError}))
}

func (e *testEnv) newUser(t *testing.T, username string) (*db.User, string) {
	t.Helper()
	u, err := e.db.CreateUser(context.Background(), username, "")
	if err != nil {
		t.Fatalf("create user: %v", err)
	}
	key, err := auth.NewKey()
	if err != nil {
		t.Fatalf("new key: %v", err)
	}
	if _, err := e.db.InsertAPIKey(context.Background(), u.ID, "test", key.Display, key.Hash); err != nil {
		t.Fatalf("insert key: %v", err)
	}
	return u, key.Secret
}

func (e *testEnv) do(method, path, key string, body io.Reader) (*http.Response, []byte) {
	e.t.Helper()
	req, err := http.NewRequest(method, e.server.URL+path, body)
	if err != nil {
		e.t.Fatalf("request: %v", err)
	}
	if key != "" {
		req.Header.Set("Authorization", "Bearer "+key)
	}
	resp, err := e.server.Client().Do(req)
	if err != nil {
		e.t.Fatalf("do: %v", err)
	}
	defer resp.Body.Close()
	data, err := io.ReadAll(resp.Body)
	if err != nil {
		e.t.Fatalf("read body: %v", err)
	}
	return resp, data
}

func TestHealthIsPublic(t *testing.T) {
	env := newTestEnv(t)
	resp, body := env.do("GET", "/healthz", "", nil)
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, body = %s", resp.StatusCode, body)
	}
}

func TestRequestsWithoutKeyAreRejected(t *testing.T) {
	env := newTestEnv(t)
	resp, _ := env.do("GET", "/v1/games", "", nil)
	if resp.StatusCode != http.StatusUnauthorized {
		t.Fatalf("status = %d, want 401", resp.StatusCode)
	}
}

func TestUnknownKeyIsRejected(t *testing.T) {
	env := newTestEnv(t)
	resp, _ := env.do("GET", "/v1/games", "gsk_totallyboguskey", nil)
	if resp.StatusCode != http.StatusUnauthorized {
		t.Fatalf("status = %d, want 401", resp.StatusCode)
	}
}

func TestMeReturnsIdentity(t *testing.T) {
	env := newTestEnv(t)
	_, key := env.newUser(t, "alice")

	resp, body := env.do("GET", "/v1/me", key, nil)
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, body = %s", resp.StatusCode, body)
	}
	var out struct {
		User struct {
			Username string `json:"username"`
		} `json:"user"`
		Key struct {
			Prefix string `json:"prefix"`
		} `json:"key"`
	}
	if err := json.Unmarshal(body, &out); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if out.User.Username != "alice" {
		t.Fatalf("username = %q", out.User.Username)
	}
	if out.Key.Prefix == "" {
		t.Fatal("the key prefix must be reported")
	}
}

func TestGameAndFileLifecycle(t *testing.T) {
	env := newTestEnv(t)
	_, key := env.newUser(t, "alice")

	// Create the game.
	resp, body := env.do("POST", "/v1/games", key, bytes.NewBufferString(`{"name":"Hollow Knight"}`))
	if resp.StatusCode != http.StatusCreated {
		t.Fatalf("create game status = %d, body = %s", resp.StatusCode, body)
	}
	var created struct {
		Game db.Game `json:"game"`
	}
	if err := json.Unmarshal(body, &created); err != nil {
		t.Fatalf("decode: %v", err)
	}
	gameID := created.Game.ID
	if gameID == "" {
		t.Fatal("the game must have an id")
	}

	// Upload a file.
	content := []byte("save data v1")
	upURL := "/v1/games/" + gameID + "/file?path=" + url.QueryEscape("saves/slot1.dat") + "&device=pc&mtime_ns=12345"
	resp, body = env.do("PUT", upURL, key, bytes.NewReader(content))
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("upload status = %d, body = %s", resp.StatusCode, body)
	}
	var up struct {
		File db.File `json:"file"`
	}
	if err := json.Unmarshal(body, &up); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if up.File.Size != int64(len(content)) {
		t.Fatalf("size = %d, want %d", up.File.Size, len(content))
	}
	if up.File.MTimeNS != 12345 {
		t.Fatalf("mtime = %d, want 12345", up.File.MTimeNS)
	}
	if up.File.Device != "pc" {
		t.Fatalf("device = %q", up.File.Device)
	}

	// The manifest must list the file.
	resp, body = env.do("GET", "/v1/games/"+gameID+"/manifest", key, nil)
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("manifest status = %d, body = %s", resp.StatusCode, body)
	}
	var manifest struct {
		Version int64    `json:"version"`
		Files   []db.File `json:"files"`
	}
	if err := json.Unmarshal(body, &manifest); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if len(manifest.Files) != 1 {
		t.Fatalf("expected 1 file, got %d", len(manifest.Files))
	}
	if manifest.Files[0].Path != "saves/slot1.dat" {
		t.Fatalf("path = %q", manifest.Files[0].Path)
	}

	// Download it back.
	dlURL := "/v1/games/" + gameID + "/file?path=" + url.QueryEscape("saves/slot1.dat")
	resp, body = env.do("GET", dlURL, key, nil)
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("download status = %d, body = %s", resp.StatusCode, body)
	}
	if !bytes.Equal(body, content) {
		t.Fatalf("content = %q, want %q", body, content)
	}
	if got := resp.Header.Get("X-File-Sha256"); got != manifest.Files[0].SHA256 {
		t.Fatalf("digest header = %q, want %q", got, manifest.Files[0].SHA256)
	}

	// A stale expectation must be rejected.
	staleURL := upURL + "&expect_sha256=" + manifest.Files[0].SHA256 + "00"
	resp, body = env.do("PUT", staleURL, key, bytes.NewReader([]byte("v2")))
	if resp.StatusCode != http.StatusConflict {
		t.Fatalf("stale upload status = %d, want 409, body = %s", resp.StatusCode, body)
	}

	// Delete the file.
	resp, body = env.do("DELETE", dlURL+"&device=pc", key, nil)
	if resp.StatusCode != http.StatusNoContent {
		t.Fatalf("delete status = %d, body = %s", resp.StatusCode, body)
	}

	resp, body = env.do("GET", "/v1/games/"+gameID+"/manifest", key, nil)
	if err := json.Unmarshal(body, &manifest); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if len(manifest.Files) != 0 {
		t.Fatalf("expected no files after the deletion, got %d", len(manifest.Files))
	}
}

func TestUsersCannotSeeEachOtherGames(t *testing.T) {
	env := newTestEnv(t)
	_, aliceKey := env.newUser(t, "alice")
	_, bobKey := env.newUser(t, "bob")

	resp, body := env.do("POST", "/v1/games", aliceKey, bytes.NewBufferString(`{"name":"Celeste"}`))
	if resp.StatusCode != http.StatusCreated {
		t.Fatalf("create game status = %d, body = %s", resp.StatusCode, body)
	}
	var created struct {
		Game db.Game `json:"game"`
	}
	json.Unmarshal(body, &created)

	resp, _ = env.do("GET", "/v1/games/"+created.Game.ID, bobKey, nil)
	if resp.StatusCode != http.StatusNotFound {
		t.Fatalf("status = %d, want 404", resp.StatusCode)
	}
	resp, _ = env.do("GET", "/v1/games/"+created.Game.ID+"/manifest", bobKey, nil)
	if resp.StatusCode != http.StatusNotFound {
		t.Fatalf("status = %d, want 404", resp.StatusCode)
	}
}

func TestPathTraversalIsRejected(t *testing.T) {
	env := newTestEnv(t)
	_, key := env.newUser(t, "alice")

	resp, body := env.do("POST", "/v1/games", key, bytes.NewBufferString(`{"name":"Game"}`))
	var created struct {
		Game db.Game `json:"game"`
	}
	json.Unmarshal(body, &created)

	bad := "/v1/games/" + created.Game.ID + "/file?path=" + url.QueryEscape("../../etc/passwd")
	resp, _ = env.do("PUT", bad, key, bytes.NewReader([]byte("x")))
	if resp.StatusCode != http.StatusBadRequest {
		t.Fatalf("status = %d, want 400", resp.StatusCode)
	}
}

func TestRevokedKeyStopsWorking(t *testing.T) {
	env := newTestEnv(t)
	u, key := env.newUser(t, "alice")

	keys, err := env.db.ListAPIKeys(context.Background(), u.ID)
	if err != nil {
		t.Fatalf("list keys: %v", err)
	}
	if err := env.db.RevokeAPIKey(context.Background(), u.ID, keys[0].ID); err != nil {
		t.Fatalf("revoke: %v", err)
	}
	resp, _ := env.do("GET", "/v1/games", key, nil)
	if resp.StatusCode != http.StatusUnauthorized {
		t.Fatalf("status = %d, want 401", resp.StatusCode)
	}
}

func TestBlobIsDeduplicated(t *testing.T) {
	env := newTestEnv(t)
	_, alice := env.newUser(t, "alice")
	_, bob := env.newUser(t, "bob")

	content := []byte("identical content")

	for _, key := range []string{alice, bob} {
		resp, body := env.do("POST", "/v1/games", key, bytes.NewBufferString(`{"name":"Game"}`))
		if resp.StatusCode != http.StatusCreated {
			t.Fatalf("create game status = %d", resp.StatusCode)
		}
		var created struct {
			Game db.Game `json:"game"`
		}
		json.Unmarshal(body, &created)

		u := "/v1/games/" + created.Game.ID + "/file?path=" + url.QueryEscape("saves/a.dat")
		resp, body = env.do("PUT", u, key, bytes.NewReader(content))
		if resp.StatusCode != http.StatusOK {
			t.Fatalf("upload status = %d, body = %s", resp.StatusCode, body)
		}
	}

	blobs := env.blobs
	digests, err := blobs.List()
	if err != nil {
		t.Fatalf("list blobs: %v", err)
	}
	if len(digests) != 1 {
		t.Fatalf("expected 1 stored blob, got %d", len(digests))
	}
}
