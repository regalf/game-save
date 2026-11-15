// Package tlscert generates and loads the self signed certificate used to
// secure the homelab server.
package tlscert

import (
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/sha256"
	"crypto/x509"
	"crypto/x509/pkix"
	"encoding/hex"
	"encoding/pem"
	"fmt"
	"math/big"
	"net"
	"os"
	"path/filepath"
	"strings"
	"time"
)

// Certificate is the material needed to serve TLS and to let a client pin it.
type Certificate struct {
	// CertPEM is the PEM encoded certificate.
	CertPEM []byte
	// KeyPEM is the PEM encoded private key.
	KeyPEM []byte
	// Fingerprint is the SHA-256 of the DER certificate, colon separated.
	Fingerprint string
	// SPKIFingerprint is the SHA-256 of the SubjectPublicKeyInfo, colon
	// separated. This is the format most tools print and pin.
	SPKIFingerprint string
}

// Generate creates a self signed certificate valid for the given subject
// alternative names. DNS names and IP addresses are told apart automatically.
func Generate(commonName string, sans []string, validityDays int) (*Certificate, error) {
	if validityDays <= 0 {
		validityDays = 3650
	}
	if len(sans) == 0 {
		sans = []string{"localhost"}
	}

	key, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		return nil, fmt.Errorf("generate key: %w", err)
	}

	serial, err := rand.Int(rand.Reader, new(big.Int).Lsh(big.NewInt(1), 128))
	if err != nil {
		return nil, fmt.Errorf("generate serial: %w", err)
	}

	notBefore := time.Now().Add(-time.Hour)
	notAfter := notBefore.AddDate(0, 0, validityDays)

	tmpl := x509.Certificate{
		SerialNumber:          serial,
		Subject:               pkix.Name{CommonName: commonName, Organization: []string{"Game Save"}},
		NotBefore:             notBefore,
		NotAfter:              notAfter,
		KeyUsage:              x509.KeyUsageDigitalSignature | x509.KeyUsageCertSign,
		ExtKeyUsage:           []x509.ExtKeyUsage{x509.ExtKeyUsageServerAuth},
		BasicConstraintsValid: true,
		IsCA:                  true,
	}

	for _, san := range sans {
		san = strings.TrimSpace(san)
		if san == "" {
			continue
		}
		if ip := net.ParseIP(san); ip != nil {
			tmpl.IPAddresses = append(tmpl.IPAddresses, ip)
			continue
		}
		tmpl.DNSNames = append(tmpl.DNSNames, san)
	}

	der, err := x509.CreateCertificate(rand.Reader, &tmpl, &tmpl, &key.PublicKey, key)
	if err != nil {
		return nil, fmt.Errorf("create certificate: %w", err)
	}
	keyDER, err := x509.MarshalPKCS8PrivateKey(key)
	if err != nil {
		return nil, fmt.Errorf("marshal key: %w", err)
	}

	certPEM := pemEncode("CERTIFICATE", der)
	keyPEM := pemEncode("PRIVATE KEY", keyDER)

	spkiDER, err := x509.MarshalPKIXPublicKey(&key.PublicKey)
	if err != nil {
		return nil, fmt.Errorf("marshal spki: %w", err)
	}

	return &Certificate{
		CertPEM:         certPEM,
		KeyPEM:          keyPEM,
		Fingerprint:     Fingerprint(der),
		SPKIFingerprint: Fingerprint(spkiDER),
	}, nil
}

// Fingerprint formats a digest as a colon separated uppercase hex string.
func Fingerprint(der []byte) string {
	sum := sha256.Sum256(der)
	parts := make([]string, len(sum))
	for i, b := range sum {
		parts[i] = strings.ToUpper(hex.EncodeToString([]byte{b}))
	}
	return strings.Join(parts, ":")
}

// Save writes the certificate and key to disk, creating directories as needed.
// The key file is written with restrictive permissions.
func (c *Certificate) Save(certPath, keyPath string) error {
	if err := os.MkdirAll(filepath.Dir(certPath), 0o755); err != nil {
		return fmt.Errorf("create cert dir: %w", err)
	}
	if err := os.MkdirAll(filepath.Dir(keyPath), 0o755); err != nil {
		return fmt.Errorf("create key dir: %w", err)
	}
	if err := os.WriteFile(certPath, c.CertPEM, 0o644); err != nil {
		return fmt.Errorf("write certificate: %w", err)
	}
	if err := os.WriteFile(keyPath, c.KeyPEM, 0o600); err != nil {
		return fmt.Errorf("write key: %w", err)
	}
	return nil
}

// SaveFingerprint writes the fingerprint of the certificate so that clients can
// be configured with it.
func (c *Certificate) SaveFingerprint(path string) error {
	if path == "" {
		return nil
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return fmt.Errorf("create fingerprint dir: %w", err)
	}
	body := fmt.Sprintf("# Game Save server certificate fingerprints\n# SHA-256 of the DER certificate:\n%s\n# SHA-256 of the SubjectPublicKeyInfo:\n%s\n",
		c.Fingerprint, c.SPKIFingerprint)
	if err := os.WriteFile(path, []byte(body), 0o644); err != nil {
		return fmt.Errorf("write fingerprint: %w", err)
	}
	return nil
}

// Load reads a certificate and key from disk.
func Load(certPath, keyPath string) (*Certificate, error) {
	certPEM, err := os.ReadFile(certPath)
	if err != nil {
		return nil, fmt.Errorf("read certificate: %w", err)
	}
	keyPEM, err := os.ReadFile(keyPath)
	if err != nil {
		return nil, fmt.Errorf("read key: %w", err)
	}
	return &Certificate{CertPEM: certPEM, KeyPEM: keyPEM}, nil
}

// pemEncode wraps DER bytes in a PEM block.
func pemEncode(blockType string, der []byte) []byte {
	return pem.EncodeToMemory(&pem.Block{Type: blockType, Bytes: der})
}
