package auth

import (
	"strings"
	"testing"
)

func TestNewKeyIsUniqueAndHashed(t *testing.T) {
	k1, err := NewKey()
	if err != nil {
		t.Fatalf("NewKey: %v", err)
	}
	k2, err := NewKey()
	if err != nil {
		t.Fatalf("NewKey: %v", err)
	}

	if k1.Secret == k2.Secret {
		t.Fatal("two generated keys are identical")
	}
	if !strings.HasPrefix(k1.Secret, KeyPrefix) {
		t.Fatalf("key %q does not start with %q", k1.Secret, KeyPrefix)
	}
	if len(k1.Secret) != len(KeyPrefix)+43 {
		t.Fatalf("unexpected key length %d", len(k1.Secret))
	}
	if k1.Hash == k1.Secret {
		t.Fatal("the hash must not be the secret")
	}
	if k1.Hash != Hash(k1.Secret) {
		t.Fatal("Hash is not deterministic")
	}
	if k1.Display == k1.Secret {
		t.Fatal("the display prefix must not be the full secret")
	}
	if !strings.HasPrefix(k1.Secret, k1.Display) {
		t.Fatalf("display %q is not a prefix of the secret", k1.Display)
	}
}

func TestParse(t *testing.T) {
	k, err := NewKey()
	if err != nil {
		t.Fatalf("NewKey: %v", err)
	}

	cases := []struct {
		name   string
		header string
		want   string
		ok     bool
	}{
		{"bearer", "Bearer " + k.Secret, k.Secret, true},
		{"bearer lower", "bearer " + k.Secret, k.Secret, true},
		{"bare", k.Secret, k.Secret, true},
		{"padded", "  Bearer   " + k.Secret + "  ", k.Secret, true},
		{"empty", "", "", false},
		{"wrong scheme", "Basic " + k.Secret, "", false},
		{"wrong prefix", "Bearer sk_live_abcdef", "", false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got, ok := Parse(tc.header)
			if ok != tc.ok {
				t.Fatalf("Parse ok = %v, want %v", ok, tc.ok)
			}
			if ok && got != tc.want {
				t.Fatalf("Parse = %q, want %q", got, tc.want)
			}
		})
	}
}
