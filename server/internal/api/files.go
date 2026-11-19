package api

import (
	"errors"
	"fmt"
	"io"
	"net/http"
	"strconv"

	"gamesave/internal/db"
)

// handleManifest returns the complete state of a game namespace.
//
// The client compares this against its local manifest to decide what to
// upload and what to download.
func (s *Server) handleManifest(w http.ResponseWriter, r *http.Request) {
	u := userFrom(r.Context())
	g, err := s.db.GameByID(r.Context(), u.ID, r.PathValue("id"))
	if err != nil {
		s.writeDBError(w, err, "could not load game")
		return
	}
	files, err := s.db.ListFiles(r.Context(), g.ID)
	if err != nil {
		s.log.Error("list files", "error", err)
		writeError(w, http.StatusInternalServerError, "internal_error", "could not list files")
		return
	}
	if files == nil {
		files = []db.File{}
	}
	writeJSON(w, http.StatusOK, map[string]any{
		"game_id": g.ID,
		"version": g.Version,
		"files":   files,
	})
}

// handleListFiles returns the files of a game.
func (s *Server) handleListFiles(w http.ResponseWriter, r *http.Request) {
	u := userFrom(r.Context())
	g, err := s.db.GameByID(r.Context(), u.ID, r.PathValue("id"))
	if err != nil {
		s.writeDBError(w, err, "could not load game")
		return
	}
	files, err := s.db.ListFiles(r.Context(), g.ID)
	if err != nil {
		s.log.Error("list files", "error", err)
		writeError(w, http.StatusInternalServerError, "internal_error", "could not list files")
		return
	}
	if files == nil {
		files = []db.File{}
	}
	writeJSON(w, http.StatusOK, map[string]any{"game_id": g.ID, "files": files})
}

// handleUploadFile stores a new version of a file.
//
// The body is the raw file content. Optional query parameters:
//
//	path         the relative path inside the game namespace (required)
//	expect_sha256  digest the client believes the server currently holds;
//	               when it does not match, the upload is rejected with 409
//	mtime_ns     modification time observed by the client, in nanoseconds
//	device       label of the device performing the upload
func (s *Server) handleUploadFile(w http.ResponseWriter, r *http.Request) {
	u := userFrom(r.Context())
	g, err := s.db.GameByID(r.Context(), u.ID, r.PathValue("id"))
	if err != nil {
		s.writeDBError(w, err, "could not load game")
		return
	}

	path, err := queryPath(r)
	if err != nil {
		writeErrorOf(w, err)
		return
	}

	maxSize := s.cfg.Limits.MaxFileSize
	if r.ContentLength > maxSize {
		writeError(w, http.StatusRequestEntityTooLarge, "too_large",
			"the file exceeds the configured maximum size")
		return
	}
	reader := http.MaxBytesReader(w, r.Body, maxSize)

	digest, size, err := s.blobs.Put(reader)
	if err != nil {
		var maxErr *http.MaxBytesError
		if errors.As(err, &maxErr) {
			writeError(w, http.StatusRequestEntityTooLarge, "too_large",
				"the file exceeds the configured maximum size")
			return
		}
		s.log.Error("store blob", "error", err)
		writeError(w, http.StatusInternalServerError, "internal_error", "could not store the file")
		return
	}

	// The account has a storage quota, when it is not zero. The file that is
	// being replaced no longer counts, so the check is on what the account
	// would hold after the upload.
	if u.Quota > 0 {
		used, err := s.db.QuotaUsed(r.Context(), u.ID)
		if err != nil {
			s.log.Error("read the used storage", "error", err)
			writeError(w, http.StatusInternalServerError, "internal_error", "could not read the used storage")
			return
		}
		var replaced int64
		if file, err := s.db.GetFile(r.Context(), g.ID, path); err == nil {
			replaced = file.Size
		}
		if used-replaced+size > u.Quota {
			writeError(w, http.StatusRequestEntityTooLarge, "quota_exceeded",
				fmt.Sprintf("the account is over its storage quota (%d of %d bytes)", used, u.Quota))
			return
		}
	}

	var mtimeNS int64
	if v := r.URL.Query().Get("mtime_ns"); v != "" {
		if n, err := strconv.ParseInt(v, 10, 64); err == nil {
			mtimeNS = n
		}
	}
	device := trimDevice(r.URL.Query().Get("device"))
	expect := r.URL.Query().Get("expect_sha256")

	file, err := s.db.PutFile(r.Context(), g.ID, path, digest, size, mtimeNS, device, expect)
	if err != nil {
		if errors.Is(err, db.ErrConflict) {
			writeError(w, http.StatusConflict, "conflict",
				"the file changed on the server since your last sync; fetch the manifest and retry")
			return
		}
		s.log.Error("put file", "error", err)
		writeError(w, http.StatusBadRequest, "bad_request", err.Error())
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"file": file})
}

// handleDownloadFile streams a stored file back to the client.
func (s *Server) handleDownloadFile(w http.ResponseWriter, r *http.Request) {
	u := userFrom(r.Context())
	g, err := s.db.GameByID(r.Context(), u.ID, r.PathValue("id"))
	if err != nil {
		s.writeDBError(w, err, "could not load game")
		return
	}
	path, err := queryPath(r)
	if err != nil {
		writeErrorOf(w, err)
		return
	}
	file, err := s.db.GetFile(r.Context(), g.ID, path)
	if err != nil {
		s.writeDBError(w, err, "could not load file")
		return
	}
	f, err := s.blobs.Open(file.SHA256)
	if err != nil {
		s.log.Error("open blob", "error", err, "sha256", file.SHA256)
		writeError(w, http.StatusInternalServerError, "internal_error", "the stored file is missing")
		return
	}
	defer f.Close()

	w.Header().Set("Content-Type", "application/octet-stream")
	w.Header().Set("Content-Length", strconv.FormatInt(file.Size, 10))
	w.Header().Set("X-File-Sha256", file.SHA256)
	w.Header().Set("X-File-Version", strconv.FormatInt(file.Version, 10))
	w.WriteHeader(http.StatusOK)
	if _, err := io.Copy(w, f); err != nil {
		s.log.Error("stream blob", "error", err)
	}
}

// handleDeleteFile removes a file from a game namespace.
func (s *Server) handleDeleteFile(w http.ResponseWriter, r *http.Request) {
	u := userFrom(r.Context())
	g, err := s.db.GameByID(r.Context(), u.ID, r.PathValue("id"))
	if err != nil {
		s.writeDBError(w, err, "could not load game")
		return
	}
	path, err := queryPath(r)
	if err != nil {
		writeErrorOf(w, err)
		return
	}
	device := trimDevice(r.URL.Query().Get("device"))
	expect := r.URL.Query().Get("expect_sha256")

	if err := s.db.DeleteFile(r.Context(), g.ID, path, device, expect); err != nil {
		if errors.Is(err, db.ErrNotFound) {
			writeError(w, http.StatusNotFound, "not_found", "the file does not exist")
			return
		}
		if errors.Is(err, db.ErrConflict) {
			writeError(w, http.StatusConflict, "conflict",
				"the file changed on the server since your last sync; fetch the manifest and retry")
			return
		}
		s.log.Error("delete file", "error", err)
		writeError(w, http.StatusInternalServerError, "internal_error", "could not delete the file")
		return
	}
	w.WriteHeader(http.StatusNoContent)
}

// handleFileVersions returns the history of a file.
func (s *Server) handleFileVersions(w http.ResponseWriter, r *http.Request) {
	u := userFrom(r.Context())
	g, err := s.db.GameByID(r.Context(), u.ID, r.PathValue("id"))
	if err != nil {
		s.writeDBError(w, err, "could not load game")
		return
	}
	path, err := queryPath(r)
	if err != nil {
		writeErrorOf(w, err)
		return
	}
	limit := 0
	if v := r.URL.Query().Get("limit"); v != "" {
		if n, err := strconv.Atoi(v); err == nil {
			limit = n
		}
	}
	versions, err := s.db.FileVersions(r.Context(), g.ID, path, limit)
	if err != nil {
		s.log.Error("file versions", "error", err)
		writeError(w, http.StatusInternalServerError, "internal_error", "could not read file history")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"game_id": g.ID, "versions": versions})
}
