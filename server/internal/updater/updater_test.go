package updater

import (
	"context"
	"fmt"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestIsNewer(t *testing.T) {
	cases := []struct {
		latest  string
		current string
		want    bool
	}{
		{"0.3.0", "0.2.0", true},
		{"v0.3.0", "v0.2.0", true},
		{"0.2.0", "0.3.0", false},
		{"0.2.0", "0.2.0", false},
		{"0.10.0", "0.9.0", true},
		{"0.2.1", "0.2.0", true},
		{"dev", "0.2.0", true},
		{"0.2.0", "0.2.0.0", true},
	}
	for _, tc := range cases {
		if got := IsNewer(tc.latest, tc.current); got != tc.want {
			t.Errorf("IsNewer(%q, %q) = %v, want %v", tc.latest, tc.current, got, tc.want)
		}
	}
}

func TestPlatform(t *testing.T) {
	if got := Platform("linux", "amd64"); got != "linux-amd64" {
		t.Fatalf("Platform = %q", got)
	}
	if got := Platform("windows", "arm64"); got != "windows-arm64" {
		t.Fatalf("Platform = %q", got)
	}
}

func TestFetch(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		fmt.Fprint(w, `{
			"version": "0.3.0",
			"tag": "v0.3.0",
			"assets": {
				"linux-amd64": {"name": "gamesave-server-linux-amd64", "sha256": "abc"}
			}
		}`)
	}))
	defer server.Close()

	release, err := fetch(context.Background(), server.URL)
	if err != nil {
		t.Fatalf("fetch: %v", err)
	}
	if release.Version != "0.3.0" {
		t.Fatalf("version = %q", release.Version)
	}
	if release.Tag != "v0.3.0" {
		t.Fatalf("tag = %q", release.Tag)
	}
	asset, ok := release.Assets["linux-amd64"]
	if !ok {
		t.Fatal("the linux-amd64 asset is missing")
	}
	if asset.Name != "gamesave-server-linux-amd64" || asset.SHA256 != "abc" {
		t.Fatalf("asset = %+v", asset)
	}
}

func TestFetchWithoutVersion(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		fmt.Fprint(w, `{"tag": "v0.3.0"}`)
	}))
	defer server.Close()

	if _, err := fetch(context.Background(), server.URL); err == nil {
		t.Fatal("a description without a version must be refused")
	}
}
