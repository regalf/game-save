// Command gamesave is the self hosted cloud save server.
package main

import (
	"context"
	"encoding/pem"
	"errors"
	"fmt"
	"log/slog"
	"net/http"
	"os"
	"os/signal"
	"path/filepath"
	"strconv"
	"strings"
	"syscall"
	"time"

	"gamesave/internal/api"
	"gamesave/internal/auth"
	"gamesave/internal/blobstore"
	"gamesave/internal/config"
	"gamesave/internal/db"
	"gamesave/internal/tlscert"
)

// version is overridden at link time with -ldflags "-X main.version=...".
var version = "dev"

func main() {
	if err := run(os.Args[1:]); err != nil {
		fmt.Fprintln(os.Stderr, "error:", err)
		os.Exit(1)
	}
}

func run(args []string) error {
	if len(args) == 0 {
		usage()
		return errors.New("no command given")
	}
	switch args[0] {
	case "serve":
		return cmdServe(args[1:])
	case "user":
		return cmdUser(args[1:])
	case "key":
		return cmdKey(args[1:])
	case "gc":
		return cmdGC(args[1:])
	case "version", "--version", "-v":
		fmt.Printf("gamesave %s\n", version)
		return nil
	case "help", "--help", "-h":
		usage()
		return nil
	default:
		usage()
		return fmt.Errorf("unknown command %q", args[0])
	}
}

func usage() {
	fmt.Println(`gamesave - self hosted cloud save for game saves

Usage:
  gamesave serve [--config <file>]     start the server
  gamesave user add <username>        create an account
  gamesave user list                   list accounts
  gamesave user disable <username>     disable an account
  gamesave key create --user <name> [--label <text>]
  gamesave key list --user <name>
  gamesave key revoke --user <name> --id <id>
  gamesave gc                          delete unreferenced blobs
  gamesave version                     print the version`)
}

// parseFlags splits "--name value" style arguments from positional ones.
func parseFlags(args []string) (flagSet, []string, error) {
	flags := flagSet{}
	var rest []string
	for i := 0; i < len(args); i++ {
		a := args[i]
		if !strings.HasPrefix(a, "--") {
			rest = append(rest, a)
			continue
		}
		name := strings.TrimPrefix(a, "--")
		if eq := strings.Index(name, "="); eq >= 0 {
			flags[name[:eq]] = name[eq+1:]
			continue
		}
		if i+1 < len(args) && !strings.HasPrefix(args[i+1], "--") {
			flags[name] = args[i+1]
			i++
			continue
		}
		flags[name] = "true"
	}
	return flags, rest, nil
}

// splitCommand separates the subcommand from its flags and positional
// arguments. The subcommand is never treated as a positional argument.
func splitCommand(args []string) (string, flagSet, []string, error) {
	if len(args) == 0 {
		return "", nil, nil, errors.New("a subcommand is required")
	}
	flags, rest, err := parseFlags(args[1:])
	if err != nil {
		return "", nil, nil, err
	}
	return args[0], flags, rest, nil
}

// openDB opens the database described by the configuration.
func openDB(cfg config.Config) (*db.DB, error) {
	if err := os.MkdirAll(cfg.Server.DataDir, 0o755); err != nil {
		return nil, fmt.Errorf("create data dir: %w", err)
	}
	database, err := db.Open(cfg.DBPath())
	if err != nil {
		return nil, err
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	if err := database.Migrate(ctx); err != nil {
		database.Close()
		return nil, err
	}
	return database, nil
}

func cmdServe(args []string) error {
	flags, _, err := parseFlags(args)
	if err != nil {
		return err
	}
	cfg, err := config.Load(flags["config"])
	if err != nil {
		return err
	}

	log := slog.New(slog.NewTextHandler(os.Stderr, &slog.HandlerOptions{Level: slog.LevelInfo}))

	database, err := openDB(cfg)
	if err != nil {
		return err
	}
	defer database.Close()

	blobs, err := blobstore.New(cfg.BlobDir())
	if err != nil {
		return err
	}

	certPath := filepath.Join(cfg.CertsDir(), "server.crt")
	keyPath := filepath.Join(cfg.CertsDir(), "server.key")

	var tlsConfig *tlsSettings
	switch cfg.Server.TLS.Mode {
	case "selfsigned":
		tlsConfig, err = ensureSelfSigned(cfg, certPath, keyPath)
		if err != nil {
			return err
		}
	case "files":
		settings, err := loadSettings(cfg.Server.TLS.CertFile, cfg.Server.TLS.KeyFile)
		if err != nil {
			return err
		}
		tlsConfig = settings
	}

	apiServer := api.New(cfg, database, blobs, log)
	if tlsConfig != nil && len(tlsConfig.certPEM) > 0 {
		apiServer = apiServer.WithCertificate(tlsConfig.certPEM, tlsConfig.fingerprint)
	}
	handler := apiServer.Handler()
	srv := &http.Server{
		Addr:              cfg.Server.Listen,
		Handler:           handler,
		ReadHeaderTimeout: 20 * time.Second,
		WriteTimeout:      0, // large uploads and downloads must not time out
		IdleTimeout:       120 * time.Second,
		MaxHeaderBytes:    1 << 20,
	}

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()

	errCh := make(chan error, 1)
	go func() {
		log.Info("Game Save server starting",
			"version", version, "listen", cfg.Server.Listen, "data_dir", cfg.Server.DataDir)
		if tlsConfig != nil {
			errCh <- srv.ListenAndServeTLS(tlsConfig.certFile, tlsConfig.keyFile)
			return
		}
		errCh <- srv.ListenAndServe()
	}()

	select {
	case <-ctx.Done():
		log.Info("shutting down")
		shutdownCtx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
		defer cancel()
		return srv.Shutdown(shutdownCtx)
	case err := <-errCh:
		if errors.Is(err, http.ErrServerClosed) {
			return nil
		}
		return err
	}
}

// tlsSettings holds the certificate paths used by the listener.
type tlsSettings struct {
	certFile    string
	keyFile     string
	certPEM     []byte
	fingerprint string
}

// ensureSelfSigned loads the existing certificate or generates a new one,
// then writes the fingerprint file so clients can pin it.
func ensureSelfSigned(cfg config.Config, certPath, keyPath string) (*tlsSettings, error) {
	fingerprintPath := cfg.Server.TLS.FingerprintFile
	if fingerprintPath != "" && !filepath.IsAbs(fingerprintPath) {
		fingerprintPath = filepath.Join(cfg.Server.DataDir, fingerprintPath)
	}

	if _, err := os.Stat(certPath); err == nil {
		if _, err := os.Stat(keyPath); err == nil {
			return loadSettings(certPath, keyPath)
		}
	}

	commonName := "gamesave.local"
	if len(cfg.Server.TLS.SANs) > 0 {
		commonName = cfg.Server.TLS.SANs[0]
	}
	cert, err := tlscert.Generate(commonName, cfg.Server.TLS.SANs, cfg.Server.TLS.ValidityDays)
	if err != nil {
		return nil, err
	}
	if err := cert.Save(certPath, keyPath); err != nil {
		return nil, err
	}
	if err := cert.SaveFingerprint(fingerprintPath); err != nil {
		return nil, err
	}
	fmt.Printf("\n  Self signed certificate created\n")
	fmt.Printf("    certificate : %s\n", certPath)
	fmt.Printf("    fingerprint : %s\n", cert.Fingerprint)
	fmt.Printf("    spki        : %s\n\n", cert.SPKIFingerprint)
	return &tlsSettings{
		certFile:    certPath,
		keyFile:     keyPath,
		certPEM:     cert.CertPEM,
		fingerprint: cert.Fingerprint,
	}, nil
}

// loadSettings reads an existing certificate and computes its fingerprint.
func loadSettings(certPath, keyPath string) (*tlsSettings, error) {
	cert, err := tlscert.Load(certPath, keyPath)
	if err != nil {
		return nil, err
	}
	der, err := pemDecodeFirst(cert.CertPEM, "CERTIFICATE")
	if err != nil {
		return nil, err
	}
	return &tlsSettings{
		certFile:    certPath,
		keyFile:     keyPath,
		certPEM:     cert.CertPEM,
		fingerprint: tlscert.Fingerprint(der),
	}, nil
}

// pemDecodeFirst returns the DER payload of the first PEM block of a type.
func pemDecodeFirst(data []byte, blockType string) ([]byte, error) {
	block, _ := pem.Decode(data)
	if block == nil {
		return nil, fmt.Errorf("no PEM block found")
	}
	if block.Type != blockType {
		return nil, fmt.Errorf("expected a %s block, got %q", blockType, block.Type)
	}
	return block.Bytes, nil
}

// flagSet holds the parsed "--name value" arguments.
type flagSet map[string]string

// cmdUser implements the "gamesave user" family of commands.
func cmdUser(args []string) error {
	sub, flags, rest, err := splitCommand(args)
	if err != nil {
		return err
	}
	cfg, err := config.Load(flags["config"])
	if err != nil {
		return err
	}
	database, err := openDB(cfg)
	if err != nil {
		return err
	}
	defer database.Close()
	ctx := context.Background()

	switch sub {
	case "add":
		if len(rest) == 0 {
			return errors.New("usage: gamesave user add <username> [--display-name <text>]")
		}
		u, err := database.CreateUser(ctx, rest[0], flags["display-name"])
		if err != nil {
			return err
		}
		fmt.Printf("user %q created (id %d)\n", u.Username, u.ID)
		return nil
	case "list":
		users, err := database.ListUsers(ctx)
		if err != nil {
			return err
		}
		if len(users) == 0 {
			fmt.Println("no users yet")
			return nil
		}
		fmt.Printf("%-6s %-20s %-10s %s\n", "ID", "USERNAME", "STATUS", "CREATED")
		for _, u := range users {
			status := "active"
			if u.Disabled {
				status = "disabled"
			}
			fmt.Printf("%-6d %-20s %-10s %s\n", u.ID, u.Username, status, u.CreatedAt)
		}
		return nil
	case "disable":
		if len(rest) == 0 {
			return errors.New("usage: gamesave user disable <username>")
		}
		u, err := database.UserByUsername(ctx, rest[0])
		if err != nil {
			return err
		}
		if err := database.SetUserDisabled(ctx, u.ID, true); err != nil {
			return err
		}
		fmt.Printf("user %q disabled\n", u.Username)
		return nil
	case "enable":
		if len(rest) == 0 {
			return errors.New("usage: gamesave user enable <username>")
		}
		u, err := database.UserByUsername(ctx, rest[0])
		if err != nil {
			return err
		}
		if err := database.SetUserDisabled(ctx, u.ID, false); err != nil {
			return err
		}
		fmt.Printf("user %q enabled\n", u.Username)
		return nil
	default:
		return fmt.Errorf("unknown user subcommand %q", sub)
	}
}

// cmdKey implements the "gamesave key" family of commands.
func cmdKey(args []string) error {
	sub, flags, _, err := splitCommand(args)
	if err != nil {
		return err
	}
	cfg, err := config.Load(flags["config"])
	if err != nil {
		return err
	}
	database, err := openDB(cfg)
	if err != nil {
		return err
	}
	defer database.Close()
	ctx := context.Background()

	username := flags["user"]
	if username == "" {
		return errors.New("--user is required")
	}
	user, err := database.UserByUsername(ctx, username)
	if err != nil {
		if errors.Is(err, db.ErrNotFound) {
			return fmt.Errorf("user %q does not exist", username)
		}
		return err
	}

	switch sub {
	case "create":
		key, err := auth.NewKey()
		if err != nil {
			return err
		}
		label := flags["label"]
		if label == "" {
			label = "device"
		}
		if _, err := database.InsertAPIKey(ctx, user.ID, label, key.Display, key.Hash); err != nil {
			return err
		}
		fmt.Printf("\n  API key for %q (%s)\n", user.Username, label)
		fmt.Printf("    %s\n\n", key.Secret)
		fmt.Println("  This is the only time the key is shown. Store it now.")
		return nil
	case "list":
		keys, err := database.ListAPIKeys(ctx, user.ID)
		if err != nil {
			return err
		}
		if len(keys) == 0 {
			fmt.Println("no keys yet")
			return nil
		}
		fmt.Printf("%-4s %-14s %-18s %-22s %s\n", "ID", "PREFIX", "LABEL", "CREATED", "LAST USED")
		for _, k := range keys {
			last := k.LastUsedAt
			if last == "" {
				last = "never"
			}
			state := "active"
			if k.RevokedAt != "" {
				state = "revoked"
			}
			fmt.Printf("%-4d %-14s %-18s %-22s %s (%s)\n", k.ID, k.Prefix, k.Label, k.CreatedAt, last, state)
		}
		return nil
	case "revoke":
		idStr := flags["id"]
		id, err := strconv.ParseInt(idStr, 10, 64)
		if err != nil {
			return fmt.Errorf("--id must be a number, got %q", idStr)
		}
		if err := database.RevokeAPIKey(ctx, user.ID, id); err != nil {
			if errors.Is(err, db.ErrNotFound) {
				return fmt.Errorf("key %d not found or already revoked", id)
			}
			return err
		}
		fmt.Printf("key %d revoked\n", id)
		return nil
	default:
		return fmt.Errorf("unknown key subcommand %q", sub)
	}
}

// cmdGC removes blobs that no game references any more.
func cmdGC(args []string) error {
	flags, _, err := parseFlags(args)
	if err != nil {
		return err
	}
	cfg, err := config.Load(flags["config"])
	if err != nil {
		return err
	}
	database, err := openDB(cfg)
	if err != nil {
		return err
	}
	defer database.Close()
	blobs, err := blobstore.New(cfg.BlobDir())
	if err != nil {
		return err
	}
	ctx := context.Background()

	referenced, err := database.ReferencedBlobs(ctx)
	if err != nil {
		return err
	}
	known, err := database.AllBlobs(ctx)
	if err != nil {
		return err
	}
	present, err := blobs.List()
	if err != nil {
		return err
	}

	var freed int64
	removed := 0
	for _, digest := range present {
		if referenced[digest] {
			continue
		}
		size, _ := blobs.Size(digest)
		if err := blobs.Remove(digest); err != nil {
			return err
		}
		if _, ok := known[digest]; ok {
			if err := database.ForgetBlob(ctx, digest); err != nil {
				return err
			}
		}
		freed += size
		removed++
	}
	fmt.Printf("removed %d unreferenced blobs (%d bytes freed)\n", removed, freed)
	return nil
}
