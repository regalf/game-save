// Package api exposes the HTTP interface of the Game Save server.
package api

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"log/slog"
	"net/http"
	"strings"
	"time"

	"gamesave/internal/auth"
	"gamesave/internal/blobstore"
	"gamesave/internal/config"
	"gamesave/internal/db"
)

// LatestVersion is the source of the latest known release, when the update
// check is enabled.
type LatestVersion interface {
	// Latest returns the version and the tag of the latest known release.
	Latest() (version, tag string)
	// URL is the page of the latest release.
	URL() string
}

// Server holds the dependencies of the HTTP layer.
type Server struct {
	cfg   config.Config
	db    *db.DB
	blobs *blobstore.Store
	log   *slog.Logger

	// anchorPEM is the trust anchor published to the clients: the certificate
	// authority that signed the server certificate.
	anchorPEM []byte
	// anchorFingerprint is the SHA-256 of the trust anchor, colon separated.
	// This is the value a client pins.
	anchorFingerprint string
	// leafFingerprint is the SHA-256 of the certificate the server presents.
	leafFingerprint string
	// latest is the latest known release, nil when the check is disabled.
	latest LatestVersion
}

// WithCertificate publishes the trust anchor and the server certificate
// fingerprints to the API.
func (s *Server) WithCertificate(anchorPEM []byte, anchorFingerprint, leafFingerprint string) *Server {
	s.anchorPEM = anchorPEM
	s.anchorFingerprint = anchorFingerprint
	s.leafFingerprint = leafFingerprint
	return s
}

// WithUpdater publishes the latest known release to the clients.
func (s *Server) WithUpdater(latest LatestVersion) *Server {
	s.latest = latest
	return s
}

// New builds an API server.
func New(cfg config.Config, database *db.DB, blobs *blobstore.Store, log *slog.Logger) *Server {
	return &Server{cfg: cfg, db: database, blobs: blobs, log: log}
}

// contextKey is the private type used for request scoped values.
type contextKey string

const (
	ctxUser contextKey = "user"
	ctxKey  contextKey = "apikey"
)

// Handler builds the fully wired HTTP handler.
func (s *Server) Handler() http.Handler {
	mux := http.NewServeMux()

	mux.HandleFunc("GET /healthz", s.handleHealth)
	mux.HandleFunc("GET /v1/cert", s.handleCert)
	mux.HandleFunc("GET /v1/me", s.authed(s.handleMe))

	mux.HandleFunc("GET /v1/games", s.authed(s.handleListGames))
	mux.HandleFunc("POST /v1/games", s.authed(s.handleCreateGame))
	mux.HandleFunc("GET /v1/games/{id}", s.authed(s.handleGetGame))
	mux.HandleFunc("PATCH /v1/games/{id}", s.authed(s.handleRenameGame))
	mux.HandleFunc("DELETE /v1/games/{id}", s.authed(s.handleDeleteGame))

	mux.HandleFunc("GET /v1/games/{id}/manifest", s.authed(s.handleManifest))
	mux.HandleFunc("GET /v1/games/{id}/files", s.authed(s.handleListFiles))
	mux.HandleFunc("GET /v1/games/{id}/file", s.authed(s.handleDownloadFile))
	mux.HandleFunc("PUT /v1/games/{id}/file", s.authed(s.handleUploadFile))
	mux.HandleFunc("DELETE /v1/games/{id}/file", s.authed(s.handleDeleteFile))
	mux.HandleFunc("GET /v1/games/{id}/file/versions", s.authed(s.handleFileVersions))

	return s.recoverer(s.logger(mux))
}

// authed wraps a handler with API key authentication.
func (s *Server) authed(next http.HandlerFunc) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if !s.cfg.Auth.RequireAPIKey {
			next(w, r)
			return
		}
		secret, ok := auth.Parse(r.Header.Get("Authorization"))
		if !ok {
			// Fall back to a dedicated header, handy for simple clients.
			secret, ok = auth.Parse(r.Header.Get("X-Api-Key"))
		}
		if !ok {
			writeError(w, http.StatusUnauthorized, "missing_api_key",
				"provide the API key in the Authorization header: 'Authorization: Bearer gsk_...'")
			return
		}

		key, user, err := s.db.APIKeyByHash(r.Context(), auth.Hash(secret))
		if err != nil {
			writeError(w, http.StatusUnauthorized, "invalid_api_key", "the API key is unknown or revoked")
			return
		}
		s.db.TouchAPIKey(r.Context(), key.ID)

		ctx := context.WithValue(r.Context(), ctxUser, user)
		ctx = context.WithValue(ctx, ctxKey, key)
		next(w, r.WithContext(ctx))
	}
}

// userFrom returns the authenticated user, never nil behind authed.
func userFrom(ctx context.Context) *db.User {
	u, _ := ctx.Value(ctxUser).(*db.User)
	return u
}

// keyFrom returns the API key used for the request.
func keyFrom(ctx context.Context) *db.APIKey {
	k, _ := ctx.Value(ctxKey).(*db.APIKey)
	return k
}

// logger writes one structured line per request.
func (s *Server) logger(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		start := time.Now()
		rec := &statusRecorder{ResponseWriter: w, status: http.StatusOK}
		next.ServeHTTP(rec, r)
		s.log.Info("request",
			"method", r.Method,
			"path", r.URL.Path,
			"status", rec.status,
			"duration_ms", time.Since(start).Milliseconds(),
			"remote", r.RemoteAddr,
		)
	})
}

// recoverer turns a panic into a 500 instead of killing the process.
func (s *Server) recoverer(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		defer func() {
			if rec := recover(); rec != nil {
				s.log.Error("panic", "error", rec, "path", r.URL.Path)
				writeError(w, http.StatusInternalServerError, "internal_error", "unexpected server error")
			}
		}()
		next.ServeHTTP(w, r)
	})
}

// statusRecorder remembers the status code written to the response.
type statusRecorder struct {
	http.ResponseWriter
	status int
}

func (r *statusRecorder) WriteHeader(code int) {
	r.status = code
	r.ResponseWriter.WriteHeader(code)
}

// errorBody is the shape of every error response.
type errorBody struct {
	Error struct {
		Code    string `json:"code"`
		Message string `json:"message"`
	} `json:"error"`
}

// writeJSON serialises a value as the response body.
func writeJSON(w http.ResponseWriter, status int, v any) {
	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	w.WriteHeader(status)
	if v == nil {
		return
	}
	enc := json.NewEncoder(w)
	enc.SetEscapeHTML(false)
	if err := enc.Encode(v); err != nil {
		return
	}
}

// writeError replies with a structured error.
func writeError(w http.ResponseWriter, status int, code, message string) {
	var body errorBody
	body.Error.Code = code
	body.Error.Message = message
	writeJSON(w, status, body)
}

// decodeJSON reads a JSON request body, rejecting unknown fields.
//
// Windows clients frequently send UTF-8 with a byte order mark, so a leading
// BOM is stripped before decoding.
func decodeJSON(r *http.Request, dst any) error {
	body, err := io.ReadAll(io.LimitReader(r.Body, 1<<20))
	if err != nil {
		return err
	}
	body = bytes.TrimPrefix(body, []byte{0xEF, 0xBB, 0xBF})
	dec := json.NewDecoder(bytes.NewReader(body))
	dec.DisallowUnknownFields()
	if err := dec.Decode(dst); err != nil {
		return err
	}
	return nil
}

// queryPath reads and validates the "path" query parameter.
func queryPath(r *http.Request) (string, error) {
	raw := r.URL.Query().Get("path")
	if raw == "" {
		return "", errBadRequest("the 'path' query parameter is required")
	}
	norm, err := db.NormalizePath(raw)
	if err != nil {
		return "", errBadRequest(err.Error())
	}
	return norm, nil
}

// httpError carries a status code together with a message.
type httpError struct {
	status  int
	code    string
	message string
}

func (e *httpError) Error() string { return e.message }

// errBadRequest builds a 400 error.
func errBadRequest(msg string) *httpError {
	return &httpError{status: http.StatusBadRequest, code: "bad_request", message: msg}
}

// errNotFound builds a 404 error.
func errNotFound(msg string) *httpError {
	return &httpError{status: http.StatusNotFound, code: "not_found", message: msg}
}

// errConflict builds a 409 error.
func errConflict(msg string) *httpError {
	return &httpError{status: http.StatusConflict, code: "conflict", message: msg}
}

// writeErrorOf maps an error onto a response.
func writeErrorOf(w http.ResponseWriter, err error) {
	var he *httpError
	if ok := asHTTPError(err, &he); ok {
		writeError(w, he.status, he.code, he.message)
		return
	}
	writeError(w, http.StatusInternalServerError, "internal_error", "unexpected server error")
}

// asHTTPError reports whether err is an *httpError and binds it.
func asHTTPError(err error, dst **httpError) bool {
	for err != nil {
		if he, ok := err.(*httpError); ok {
			*dst = he
			return true
		}
		u, ok := err.(interface{ Unwrap() error })
		if !ok {
			return false
		}
		err = u.Unwrap()
	}
	return false
}

// trimDevice normalises the device label sent by a client.
func trimDevice(s string) string {
	s = strings.TrimSpace(s)
	if len(s) > 64 {
		s = s[:64]
	}
	return s
}
