// Package config handles loading and validating the server configuration.
package config

import (
	"fmt"
	"os"
	"path/filepath"
	"strconv"
	"strings"

	"gopkg.in/yaml.v3"
)

// Config is the root configuration document.
type Config struct {
	Server ServerConfig `yaml:"server"`
	Auth   AuthConfig   `yaml:"auth"`
	Limits LimitsConfig `yaml:"limits"`
	Update UpdateConfig `yaml:"update"`
}

// ServerConfig holds network and storage settings.
type ServerConfig struct {
	// Listen is the address the HTTPS listener binds to, e.g. ":8443".
	Listen string `yaml:"listen"`
	// DataDir is where the SQLite database and the blob store live.
	DataDir string `yaml:"data_dir"`
	TLS     TLSConfig `yaml:"tls"`
}

// TLSConfig describes how the server secures its listener.
//
// Mode is one of:
//   - "selfsigned": generate and use a self-signed certificate (homelab default)
//   - "files":      use the certificate/key pair at CertFile/KeyFile
//   - "off":        plain HTTP (only for reverse-proxy setups or local testing)
type TLSConfig struct {
	Mode          string   `yaml:"mode"`
	CertFile      string   `yaml:"cert_file"`
	KeyFile       string   `yaml:"key_file"`
	SANs          []string `yaml:"san"`
	ValidityDays  int      `yaml:"validity_days"`
	FingerprintFile string `yaml:"fingerprint_file"`
}

// AuthConfig holds authentication settings.
type AuthConfig struct {
	// RequireAPIKey rejects anonymous requests when true.
	RequireAPIKey bool `yaml:"require_api_key"`
}

// LimitsConfig holds request size limits.
type LimitsConfig struct {
	// MaxFileSize is the largest single file the server accepts, in bytes.
	MaxFileSize int64 `yaml:"max_file_size"`
}

// UpdateConfig holds the update check.
//
// The server only looks: it never updates itself. To update, stop the server
// and run `gamesave update`.
type UpdateConfig struct {
	// Check asks the server to look for a newer release once in a while.
	Check bool `yaml:"check"`
	// IntervalHours is how often the server looks, in hours.
	IntervalHours int `yaml:"interval_hours"`
	// Repo is the GitHub repository that publishes the releases.
	Repo string `yaml:"repo"`
}

// Default returns the configuration used when nothing is specified.
func Default() Config {
	return Config{
		Server: ServerConfig{
			Listen:  ":8443",
			DataDir: "./data",
			TLS: TLSConfig{
				Mode:           "selfsigned",
				SANs:           []string{"localhost", "127.0.0.1"},
				ValidityDays:   3650,
				FingerprintFile: "certs/fingerprint.txt",
			},
		},
		Auth:   AuthConfig{RequireAPIKey: true},
		Limits: LimitsConfig{MaxFileSize: 1 << 30}, // 1 GiB
		Update: UpdateConfig{
			Check:         false,
			IntervalHours: 24,
			Repo:          "regalf/game-save",
		},
	}
}

// Load reads the configuration from path, applying defaults, then overlays
// environment variables. An empty path means "defaults plus environment".
func Load(path string) (Config, error) {
	cfg := Default()

	if path != "" {
		raw, err := os.ReadFile(path)
		if err != nil {
			return cfg, fmt.Errorf("read config %s: %w", path, err)
		}
		dec := yaml.NewDecoder(strings.NewReader(string(raw)))
		dec.KnownFields(true)
		if err := dec.Decode(&cfg); err != nil {
			return cfg, fmt.Errorf("parse config %s: %w", path, err)
		}
	}

	applyEnv(&cfg)

	if err := cfg.validate(); err != nil {
		return cfg, err
	}
	return cfg, nil
}

// applyEnv overlays environment variables over the loaded configuration.
func applyEnv(cfg *Config) {
	if v := os.Getenv("GAMESAVE_LISTEN"); v != "" {
		cfg.Server.Listen = v
	}
	if v := os.Getenv("GAMESAVE_DATA_DIR"); v != "" {
		cfg.Server.DataDir = v
	}
	if v := os.Getenv("GAMESAVE_TLS_MODE"); v != "" {
		cfg.Server.TLS.Mode = v
	}
	if v := os.Getenv("GAMESAVE_CERT_FILE"); v != "" {
		cfg.Server.TLS.CertFile = v
	}
	if v := os.Getenv("GAMESAVE_KEY_FILE"); v != "" {
		cfg.Server.TLS.KeyFile = v
	}
	if v := os.Getenv("GAMESAVE_FINGERPRINT_FILE"); v != "" {
		cfg.Server.TLS.FingerprintFile = v
	}
	if v := os.Getenv("GAMESAVE_MAX_FILE_SIZE"); v != "" {
		var n int64
		if _, err := fmt.Sscanf(v, "%d", &n); err == nil && n > 0 {
			cfg.Limits.MaxFileSize = n
		}
	}
	if v := os.Getenv("GAMESAVE_SANS"); v != "" {
		var sans []string
		for _, s := range strings.Split(v, ",") {
			if s = strings.TrimSpace(s); s != "" {
				sans = append(sans, s)
			}
		}
		if len(sans) > 0 {
			cfg.Server.TLS.SANs = sans
		}
	}
	if v := os.Getenv("GAMESAVE_UPDATE_CHECK"); v != "" {
		if b, err := strconv.ParseBool(v); err == nil {
			cfg.Update.Check = b
		}
	}
	if v := os.Getenv("GAMESAVE_UPDATE_INTERVAL_HOURS"); v != "" {
		if n, err := strconv.Atoi(v); err == nil && n > 0 {
			cfg.Update.IntervalHours = n
		}
	}
	if v := os.Getenv("GAMESAVE_UPDATE_REPO"); v != "" {
		cfg.Update.Repo = v
	}
}

// validate checks the configuration for internal consistency.
func (cfg Config) validate() error {
	switch cfg.Server.TLS.Mode {
	case "selfsigned", "files", "off":
	default:
		return fmt.Errorf("server.tls.mode must be one of selfsigned|files|off, got %q", cfg.Server.TLS.Mode)
	}
	if cfg.Server.TLS.Mode == "files" {
		if cfg.Server.TLS.CertFile == "" || cfg.Server.TLS.KeyFile == "" {
			return fmt.Errorf("server.tls.cert_file and server.tls.key_file are required when mode is 'files'")
		}
	}
	if cfg.Server.DataDir == "" {
		return fmt.Errorf("server.data_dir must not be empty")
	}
	if cfg.Limits.MaxFileSize <= 0 {
		return fmt.Errorf("limits.max_file_size must be positive")
	}
	if cfg.Update.IntervalHours <= 0 {
		return fmt.Errorf("update.interval_hours must be positive")
	}
	if cfg.Update.Repo == "" {
		return fmt.Errorf("update.repo must not be empty")
	}
	return nil
}

// CertsDir is the directory holding the TLS material inside the data dir.
func (cfg Config) CertsDir() string {
	return filepath.Join(cfg.Server.DataDir, "certs")
}

// BlobDir is the directory holding the content addressed blob store.
func (cfg Config) BlobDir() string {
	return filepath.Join(cfg.Server.DataDir, "blobs")
}

// DBPath is the path of the SQLite database file.
func (cfg Config) DBPath() string {
	return filepath.Join(cfg.Server.DataDir, "gamesave.db")
}
