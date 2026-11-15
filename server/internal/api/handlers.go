package api

import (
	"errors"
	"net/http"
	"runtime"

	"gamesave/internal/db"
)

// Version is the build version of the server, overridden at link time.
var Version = "dev"

// handleHealth is the unauthenticated liveness probe.
func (s *Server) handleHealth(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, map[string]any{
		"status":  "ok",
		"version": Version,
		"go":      runtime.Version(),
	})
}

// handleCert publishes the trust anchor of the server.
//
// A client fetches it once, shows the fingerprint to the user and pins it:
// from then on only certificates signed by that authority are trusted.
func (s *Server) handleCert(w http.ResponseWriter, r *http.Request) {
	if len(s.anchorPEM) == 0 {
		writeError(w, http.StatusNotFound, "not_found", "the server does not expose a certificate")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{
		"certificate":       string(s.anchorPEM),
		"fingerprint":       s.anchorFingerprint,
		"server_fingerprint": s.leafFingerprint,
	})
}

// handleMe returns the identity behind the presented API key.
func (s *Server) handleMe(w http.ResponseWriter, r *http.Request) {
	u := userFrom(r.Context())
	k := keyFrom(r.Context())
	writeJSON(w, http.StatusOK, map[string]any{
		"user": map[string]any{
			"id":           u.ID,
			"username":     u.Username,
			"display_name": u.DisplayName,
			"created_at":   u.CreatedAt,
		},
		"key": map[string]any{
			"id":         k.ID,
			"label":      k.Label,
			"prefix":     k.Prefix,
			"created_at": k.CreatedAt,
			"last_used_at": k.LastUsedAt,
		},
	})
}

// handleListGames returns the games owned by the caller.
func (s *Server) handleListGames(w http.ResponseWriter, r *http.Request) {
	u := userFrom(r.Context())
	games, err := s.db.ListGames(r.Context(), u.ID)
	if err != nil {
		s.log.Error("list games", "error", err)
		writeError(w, http.StatusInternalServerError, "internal_error", "could not list games")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"games": games})
}

// handleCreateGame registers a new sync namespace.
func (s *Server) handleCreateGame(w http.ResponseWriter, r *http.Request) {
	u := userFrom(r.Context())
	var req struct {
		Name string `json:"name"`
		Slug string `json:"slug"`
	}
	if err := decodeJSON(r, &req); err != nil {
		writeError(w, http.StatusBadRequest, "bad_request", "invalid JSON body: "+err.Error())
		return
	}
	g, err := s.db.CreateGame(r.Context(), u.ID, req.Name, req.Slug)
	if err != nil {
		writeError(w, http.StatusBadRequest, "bad_request", err.Error())
		return
	}
	writeJSON(w, http.StatusCreated, map[string]any{"game": g})
}

// handleGetGame returns a single game owned by the caller.
func (s *Server) handleGetGame(w http.ResponseWriter, r *http.Request) {
	u := userFrom(r.Context())
	g, err := s.db.GameByID(r.Context(), u.ID, r.PathValue("id"))
	if err != nil {
		s.writeDBError(w, err, "could not load game")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"game": g})
}

// handleRenameGame changes the display name of a game.
func (s *Server) handleRenameGame(w http.ResponseWriter, r *http.Request) {
	u := userFrom(r.Context())
	var req struct {
		Name string `json:"name"`
	}
	if err := decodeJSON(r, &req); err != nil {
		writeError(w, http.StatusBadRequest, "bad_request", "invalid JSON body: "+err.Error())
		return
	}
	if err := s.db.RenameGame(r.Context(), u.ID, r.PathValue("id"), req.Name); err != nil {
		s.writeDBError(w, err, "could not rename game")
		return
	}
	g, err := s.db.GameByID(r.Context(), u.ID, r.PathValue("id"))
	if err != nil {
		s.writeDBError(w, err, "could not load game")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"game": g})
}

// handleDeleteGame removes a game and its files.
func (s *Server) handleDeleteGame(w http.ResponseWriter, r *http.Request) {
	u := userFrom(r.Context())
	if err := s.db.DeleteGame(r.Context(), u.ID, r.PathValue("id")); err != nil {
		s.writeDBError(w, err, "could not delete game")
		return
	}
	w.WriteHeader(http.StatusNoContent)
}

// writeDBError maps a database error onto an HTTP response.
func (s *Server) writeDBError(w http.ResponseWriter, err error, context string) {
	switch {
	case errors.Is(err, db.ErrNotFound):
		writeError(w, http.StatusNotFound, "not_found", "the requested resource does not exist")
	case errors.Is(err, db.ErrConflict):
		writeError(w, http.StatusConflict, "conflict", "the resource changed on the server since your last sync")
	default:
		s.log.Error(context, "error", err)
		writeError(w, http.StatusInternalServerError, "internal_error", context)
	}
}
