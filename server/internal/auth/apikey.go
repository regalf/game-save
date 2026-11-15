// Package auth issues and verifies the API keys used to authenticate clients.
package auth

import (
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"fmt"
	"strings"
)

// KeyPrefix identifies Game Save API keys.
const KeyPrefix = "gsk_"

// prefixLen is the number of secret characters kept in clear for display.
const prefixLen = 8

// Key is a freshly issued API key. The secret is returned to the caller once
// and never stored: only its SHA-256 hash reaches the database.
type Key struct {
	// Secret is the full key, shown once at creation time.
	Secret string
	// Display is the non secret prefix, used to identify a key later.
	Display string
	// Hash is the hex encoded SHA-256 of the secret, safe to persist.
	Hash string
}

// NewKey generates a new API key.
func NewKey() (*Key, error) {
	raw := make([]byte, 32)
	if _, err := rand.Read(raw); err != nil {
		return nil, fmt.Errorf("read random: %w", err)
	}
	secret := KeyPrefix + base64.RawURLEncoding.EncodeToString(raw)
	return &Key{
		Secret: secret,
		Display: Display(secret),
		Hash:   Hash(secret),
	}, nil
}

// Hash returns the hex encoded SHA-256 of a key.
func Hash(secret string) string {
	sum := sha256.Sum256([]byte(secret))
	return hex.EncodeToString(sum[:])
}

// Display extracts the non secret prefix of a key.
func Display(secret string) string {
	body := strings.TrimPrefix(secret, KeyPrefix)
	if len(body) <= prefixLen {
		return KeyPrefix + body
	}
	return KeyPrefix + body[:prefixLen]
}

// Parse extracts the secret from an Authorization header value.
//
// It accepts both "Bearer <key>" and a bare key, so that simple clients can
// authenticate without building a header.
func Parse(header string) (string, bool) {
	h := strings.TrimSpace(header)
	if h == "" {
		return "", false
	}
	if len(h) >= 7 && strings.EqualFold(h[:7], "Bearer ") {
		h = strings.TrimSpace(h[7:])
	}
	if h == "" || !strings.HasPrefix(h, KeyPrefix) {
		return "", false
	}
	return h, true
}
