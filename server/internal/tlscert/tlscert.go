// Package tlscert generates and loads the TLS material used to secure the
// homelab server.
//
// The server runs a small local certificate authority: the authority signs
// the certificate the server presents. A client pins the authority by
// fingerprint and trusts nothing else, which gives proper certificate
// validation without a public certificate authority.
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

// Certificate is the material needed to serve TLS.
type Certificate struct {
	// CertPEM is the PEM encoded certificate.
	CertPEM []byte
	// KeyPEM is the PEM encoded private key.
	KeyPEM []byte
	// Fingerprint is the SHA-256 of the DER certificate, colon separated.
	Fingerprint string
	// SPKIFingerprint is the SHA-256 of the SubjectPublicKeyInfo.
	SPKIFingerprint string
}

// Authority is a local certificate authority that signs server certificates.
type Authority struct {
	// CertPEM is the PEM encoded certificate of the authority.
	CertPEM []byte
	// KeyPEM is the PEM encoded private key of the authority.
	KeyPEM []byte

	cert *x509.Certificate
	key  *ecdsa.PrivateKey
}

// GenerateAuthority creates a self signed certificate authority.
func GenerateAuthority(commonName string, validityDays int) (*Authority, error) {
	if validityDays <= 0 {
		validityDays = 3650
	}
	key, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		return nil, fmt.Errorf("generate key: %w", err)
	}
	serial, err := newSerial()
	if err != nil {
		return nil, err
	}
	now := time.Now()
	tmpl := x509.Certificate{
		SerialNumber:          serial,
		Subject:               pkix.Name{CommonName: commonName, Organization: []string{"Game Save"}},
		NotBefore:             now.Add(-time.Hour),
		NotAfter:              now.AddDate(0, 0, validityDays),
		KeyUsage:              x509.KeyUsageCertSign | x509.KeyUsageCRLSign | x509.KeyUsageDigitalSignature,
		BasicConstraintsValid: true,
		IsCA:                  true,
		MaxPathLen:            0,
	}
	der, err := x509.CreateCertificate(rand.Reader, &tmpl, &tmpl, &key.PublicKey, key)
	if err != nil {
		return nil, fmt.Errorf("create authority: %w", err)
	}
	keyDER, err := x509.MarshalPKCS8PrivateKey(key)
	if err != nil {
		return nil, fmt.Errorf("marshal key: %w", err)
	}
	cert, err := x509.ParseCertificate(der)
	if err != nil {
		return nil, fmt.Errorf("parse authority: %w", err)
	}
	return &Authority{
		CertPEM: pemEncode("CERTIFICATE", der),
		KeyPEM:  pemEncode("PRIVATE KEY", keyDER),
		cert:    cert,
		key:     key,
	}, nil
}

// Sign issues a server certificate for the given subject alternative names.
//
// DNS names and IP addresses are told apart automatically.
func (a *Authority) Sign(commonName string, sans []string, validityDays int) (*Certificate, error) {
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
	serial, err := newSerial()
	if err != nil {
		return nil, err
	}

	notBefore := time.Now().Add(-time.Hour)
	notAfter := notBefore.AddDate(0, 0, validityDays)
	// The certificate cannot outlive the authority that signs it.
	if notAfter.After(a.cert.NotAfter) {
		notAfter = a.cert.NotAfter
	}

	tmpl := x509.Certificate{
		SerialNumber:          serial,
		Subject:               pkix.Name{CommonName: commonName, Organization: []string{"Game Save"}},
		NotBefore:             notBefore,
		NotAfter:              notAfter,
		KeyUsage:              x509.KeyUsageDigitalSignature | x509.KeyUsageKeyEncipherment,
		ExtKeyUsage:           []x509.ExtKeyUsage{x509.ExtKeyUsageServerAuth},
		BasicConstraintsValid: true,
		IsCA:                  false,
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

	der, err := x509.CreateCertificate(rand.Reader, &tmpl, a.cert, &key.PublicKey, a.key)
	if err != nil {
		return nil, fmt.Errorf("create certificate: %w", err)
	}
	keyDER, err := x509.MarshalPKCS8PrivateKey(key)
	if err != nil {
		return nil, fmt.Errorf("marshal key: %w", err)
	}
	spkiDER, err := x509.MarshalPKIXPublicKey(&key.PublicKey)
	if err != nil {
		return nil, fmt.Errorf("marshal spki: %w", err)
	}

	return &Certificate{
		CertPEM:         pemEncode("CERTIFICATE", der),
		KeyPEM:          pemEncode("PRIVATE KEY", keyDER),
		Fingerprint:     Fingerprint(der),
		SPKIFingerprint: Fingerprint(spkiDER),
	}, nil
}

// LoadAuthority reads an authority from disk.
func LoadAuthority(certPath, keyPath string) (*Authority, error) {
	certPEM, err := os.ReadFile(certPath)
	if err != nil {
		return nil, fmt.Errorf("read authority: %w", err)
	}
	keyPEM, err := os.ReadFile(keyPath)
	if err != nil {
		return nil, fmt.Errorf("read authority key: %w", err)
	}
	block, _ := pem.Decode(certPEM)
	if block == nil {
		return nil, fmt.Errorf("the authority certificate is not valid PEM")
	}
	cert, err := x509.ParseCertificate(block.Bytes)
	if err != nil {
		return nil, fmt.Errorf("parse authority: %w", err)
	}
	keyBlock, _ := pem.Decode(keyPEM)
	if keyBlock == nil {
		return nil, fmt.Errorf("the authority key is not valid PEM")
	}
	parsed, err := x509.ParsePKCS8PrivateKey(keyBlock.Bytes)
	if err != nil {
		return nil, fmt.Errorf("parse authority key: %w", err)
	}
	key, ok := parsed.(*ecdsa.PrivateKey)
	if !ok {
		return nil, fmt.Errorf("the authority key is not an ECDSA key")
	}
	return &Authority{CertPEM: certPEM, KeyPEM: keyPEM, cert: cert, key: key}, nil
}

// Save writes the authority to disk.
func (a *Authority) Save(certPath, keyPath string) error {
	if err := os.MkdirAll(filepath.Dir(certPath), 0o755); err != nil {
		return fmt.Errorf("create cert dir: %w", err)
	}
	if err := os.WriteFile(certPath, a.CertPEM, 0o644); err != nil {
		return fmt.Errorf("write authority: %w", err)
	}
	if err := os.WriteFile(keyPath, a.KeyPEM, 0o600); err != nil {
		return fmt.Errorf("write authority key: %w", err)
	}
	return nil
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

// FingerprintOfPEM returns the fingerprint of the first certificate of a PEM.
func FingerprintOfPEM(data []byte) (string, error) {
	block, _ := pem.Decode(data)
	if block == nil {
		return "", fmt.Errorf("no PEM block found")
	}
	return Fingerprint(block.Bytes), nil
}

// FingerprintOfPEMOrEmpty returns the fingerprint of the first certificate of
// a PEM, or an empty string when it cannot be read.
func FingerprintOfPEMOrEmpty(data []byte) string {
	fingerprint, err := FingerprintOfPEM(data)
	if err != nil {
		return ""
	}
	return fingerprint
}

// Save writes the certificate and key to disk, creating directories as needed.
// The key file is written with restrictive permissions.
func (c *Certificate) Save(certPath, keyPath string) error {
	if err := os.MkdirAll(filepath.Dir(certPath), 0o755); err != nil {
		return fmt.Errorf("create cert dir: %w", err)
	}
	if err := os.WriteFile(certPath, c.CertPEM, 0o644); err != nil {
		return fmt.Errorf("write certificate: %w", err)
	}
	if err := os.WriteFile(keyPath, c.KeyPEM, 0o600); err != nil {
		return fmt.Errorf("write key: %w", err)
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

// newSerial returns a random serial number.
func newSerial() (*big.Int, error) {
	serial, err := rand.Int(rand.Reader, new(big.Int).Lsh(big.NewInt(1), 128))
	if err != nil {
		return nil, fmt.Errorf("generate serial: %w", err)
	}
	return serial, nil
}

// pemEncode wraps DER bytes in a PEM block.
func pemEncode(blockType string, der []byte) []byte {
	return pem.EncodeToMemory(&pem.Block{Type: blockType, Bytes: der})
}
