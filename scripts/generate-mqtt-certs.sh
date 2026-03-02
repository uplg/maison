#!/bin/bash

# Generate self-signed TLS certificates for the Mosquitto MQTT broker.
# Meross MSS310 plugs require TLS but do NOT validate the certificate,
# so a simple self-signed CA is sufficient.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CERT_DIR="$SCRIPT_DIR/../mosquitto/certs"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${GREEN}Generating MQTT broker TLS certificates${NC}"

mkdir -p "$CERT_DIR"

# Skip if certs already exist
if [ -f "$CERT_DIR/server.pem" ] && [ -f "$CERT_DIR/server-key.pem" ] && [ -f "$CERT_DIR/ca.pem" ]; then
    echo -e "${YELLOW}Certificates already exist in $CERT_DIR — skipping${NC}"
    echo "Delete them manually and re-run this script to regenerate."
    exit 0
fi

# Generate CA key + cert
openssl req -x509 -newkey rsa:2048 -nodes \
    -keyout "$CERT_DIR/ca-key.pem" \
    -out "$CERT_DIR/ca.pem" \
    -days 3650 \
    -subj "/CN=Home Monitor MQTT CA" \
    2>/dev/null

# Generate server key + CSR
openssl req -newkey rsa:2048 -nodes \
    -keyout "$CERT_DIR/server-key.pem" \
    -out "$CERT_DIR/server.csr" \
    -subj "/CN=localhost" \
    2>/dev/null

# Sign server cert with CA (valid for localhost + LAN IPs)
openssl x509 -req \
    -in "$CERT_DIR/server.csr" \
    -CA "$CERT_DIR/ca.pem" \
    -CAkey "$CERT_DIR/ca-key.pem" \
    -CAcreateserial \
    -out "$CERT_DIR/server.pem" \
    -days 3650 \
    -extfile <(printf "subjectAltName=DNS:localhost,IP:127.0.0.1,IP:192.168.1.165") \
    2>/dev/null

# Clean up intermediate files
rm -f "$CERT_DIR/server.csr" "$CERT_DIR/ca.srl"

echo -e "${GREEN}MQTT TLS certificates generated:${NC}"
echo "  - $CERT_DIR/ca.pem        (CA certificate)"
echo "  - $CERT_DIR/server.pem    (server certificate)"
echo "  - $CERT_DIR/server-key.pem (server private key)"
