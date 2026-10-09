#!/usr/bin/env bash
#
# Throwaway CA and node certificate for the TLS fixture. Regenerate by deleting
# docker/tls/certs; the integration tier reads them from there.
#
# The node certificate names 127.0.0.1 and cassandra.test; a client that
# dials the address verifies against the first, `server_name` the second.

set -euo pipefail

OUT="$(cd "$(dirname "$0")" && pwd)/certs"
mkdir -p "$OUT"
cd "$OUT"

# For the `local-tls-cqlshrc` connection: login and TLS from one cqlsh file.
# Absolute paths, since cqlsh and lazycassandra resolve relative ones differently.
write_cqlshrc() {
    printf '[PlainTextAuthProvider]\nusername = cassandra\npassword = cassandra\n' > credentials
    chmod 600 credentials
    printf '[auth_provider]\nmodule = cassandra.auth\nclassname = PlainTextAuthProvider\nusername = cassandra\n\n[authentication]\ncredentials = %s/credentials\n\n[connection]\nssl = true\n\n[ssl]\ncertfile = %s/ca.pem\nvalidate = true\n' "$OUT" "$OUT" > cqlshrc
}

if [ -f ca.pem ]; then
    write_cqlshrc
    exit 0
fi

openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out ca.key
openssl req -x509 -new -key ca.key -sha256 -days 3650 -subj "/CN=lc-fixture-ca" \
    -addext "basicConstraints=critical,CA:TRUE" \
    -addext "keyUsage=critical,keyCertSign,cRLSign" -out ca.pem

# RSA: Cassandra's PEM reader rejects the EC PKCS#8 key.
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out node.key
openssl req -new -key node.key -subj "/CN=cassandra" -out node.csr
openssl x509 -req -in node.csr -CA ca.pem -CAkey ca.key -CAcreateserial -sha256 -days 3650 \
    -extfile <(printf 'subjectAltName=IP:127.0.0.1,DNS:cassandra.test\nextendedKeyUsage=serverAuth\nbasicConstraints=CA:FALSE\n') \
    -out node.pem

# Unrelated CA, for the "untrusted issuer" case.
openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out other-ca.key
openssl req -x509 -new -key other-ca.key -sha256 -days 3650 -subj "/CN=lc-other-ca" \
    -addext "basicConstraints=critical,CA:TRUE" \
    -addext "keyUsage=critical,keyCertSign,cRLSign" -out other-ca.pem

# PEMBasedSslContextFactory reads the key and chain from one `keystore` file.
cat node.key node.pem > node-bundle.pem

rm -f node.csr ca.srl
chmod 644 ./*.pem ./*.key

write_cqlshrc
