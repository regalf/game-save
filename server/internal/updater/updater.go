// Package updater reads the release description of the project.
//
// The description is a small JSON file attached to every release,
// latest.json, which holds the version and the sha256 of every server binary.
// The server reads it to tell the client that a newer version exists; the
// command `gamesave update` reads it to download and verify the binary before
// replacing the running executable.
package updater

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"strconv"
	"strings"
	"sync"
	"time"
)

// Asset is one file of the release, for one platform.
type Asset struct {
	// Name is the file name of the asset in the release.
	Name string `json:"name"`
	// SHA256 is the hex encoded SHA-256 of the file.
	SHA256 string `json:"sha256"`
}

// Release is the content of latest.json.
type Release struct {
	// Version is the version without the leading v, for example "0.3.0".
	Version string `json:"version"`
	// Tag is the git tag, for example "v0.3.0".
	Tag string `json:"tag"`
	// Assets maps a platform key ("linux-amd64", "windows-amd64", ...) to its
	// server binary.
	Assets map[string]Asset `json:"assets"`
	// Clients maps a platform key to the companion app binary. It is optional:
	// the server does not update the client, the file is there for the release
	// page and for whoever wants to script the download.
	Clients map[string]Asset `json:"clients,omitempty"`
}

// Fetch reads the description of the latest release.
func Fetch(ctx context.Context, repo string) (*Release, error) {
	url := fmt.Sprintf("https://github.com/%s/releases/latest/download/latest.json", repo)
	return fetch(ctx, url)
}

// FetchTag reads the description of a specific release.
func FetchTag(ctx context.Context, repo, tag string) (*Release, error) {
	url := fmt.Sprintf("https://github.com/%s/releases/download/%s/latest.json", repo, tag)
	return fetch(ctx, url)
}

// fetch downloads and decodes a release description.
func fetch(ctx context.Context, url string) (*Release, error) {
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return nil, err
	}
	request.Header.Set("User-Agent", "gamesave")

	client := &http.Client{Timeout: 30 * time.Second}
	response, err := client.Do(request)
	if err != nil {
		return nil, err
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		if response.StatusCode == http.StatusNotFound ||
			response.StatusCode == http.StatusUnauthorized ||
			response.StatusCode == http.StatusForbidden {
			return nil, fmt.Errorf(
				"HTTP %d: the repository may be private, or the release has no latest.json",
				response.StatusCode,
			)
		}
		return nil, fmt.Errorf("HTTP %d", response.StatusCode)
	}

	var release Release
	if err := json.NewDecoder(response.Body).Decode(&release); err != nil {
		return nil, fmt.Errorf("decode the release description: %w", err)
	}
	if release.Version == "" {
		return nil, fmt.Errorf("the release description has no version")
	}
	if release.Tag == "" {
		release.Tag = "v" + release.Version
	}
	return &release, nil
}

// Platform returns the key of the assets for an operating system and
// architecture, for example "linux-amd64".
func Platform(goos, goarch string) string {
	return goos + "-" + goarch
}

// IsNewer reports whether latest is a version above current.
//
// The versions are "major.minor.patch", with an optional leading v. When one
// of them does not parse, they are compared as strings: the answer is only
// used to decide whether to tell the user, so a wrong answer is not a
// problem.
func IsNewer(latest, current string) bool {
	left, okLeft := parseVersion(latest)
	right, okRight := parseVersion(current)
	if !okLeft || !okRight {
		return latest != current
	}
	for i := 0; i < len(left); i++ {
		if left[i] != right[i] {
			return left[i] > right[i]
		}
	}
	return false
}

// parseVersion parses "v0.3.0" into [0, 3, 0].
func parseVersion(version string) ([3]int, bool) {
	var out [3]int
	text := strings.TrimPrefix(strings.TrimSpace(version), "v")
	parts := strings.SplitN(text, ".", 3)
	if len(parts) != len(out) {
		return out, false
	}
	for i, part := range parts {
		value, err := strconv.Atoi(part)
		if err != nil {
			return out, false
		}
		out[i] = value
	}
	return out, true
}

// Checker keeps the latest known release, refreshing it in the background.
//
// The check is best effort: a failure is silent, and the server works without
// it. It is off unless the configuration says otherwise.
type Checker struct {
	repo     string
	interval time.Duration

	mu      sync.Mutex
	version string
	tag     string
}

// NewChecker builds a checker for a repository.
func NewChecker(repo string, interval time.Duration) *Checker {
	return &Checker{repo: repo, interval: interval}
}

// Start refreshes the version now and then, until the context is done.
func (c *Checker) Start(ctx context.Context) {
	c.refresh(ctx)
	go func() {
		ticker := time.NewTicker(c.interval)
		defer ticker.Stop()
		for {
			select {
			case <-ctx.Done():
				return
			case <-ticker.C:
				c.refresh(ctx)
			}
		}
	}()
}

// refresh reads the latest release and remembers it.
func (c *Checker) refresh(ctx context.Context) {
	release, err := Fetch(ctx, c.repo)
	if err != nil {
		return
	}
	c.mu.Lock()
	c.version = release.Version
	c.tag = release.Tag
	c.mu.Unlock()
}

// Latest returns the latest known version and tag, empty when nothing was read
// yet.
func (c *Checker) Latest() (version, tag string) {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.version, c.tag
}

// URL is the page of the latest release, the one the user opens to read the
// notes.
func (c *Checker) URL() string {
	return fmt.Sprintf("https://github.com/%s/releases/latest", c.repo)
}
