#!/bin/sh

set -e

usage() {
	echo "Usage: $0 <server|client> [common-name] [ip...]" >&2
	echo "  server [cn=kids] [ip=127.0.0.1]...  generate a server certificate/key as [controller.tls], with SAN DNS:<cn> plus IP:<ip> for each given ip" >&2
	echo "  client [cn=keycloak]                generate a client certificate as a [[controller.tls.client_auth.clients]] entry" >&2
	exit 1
}

print_pem_block() {
	key="$1"
	file="$2"
	printf '%s = """\n' "$key"
	cat "$file"
	printf '"""\n'
}

[ "$#" -ge 1 ] || usage
kind="$1"
shift

tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT
key_file="$tmpdir/key.pem"
cert_file="$tmpdir/cert.pem"

case "$kind" in
server)
	if [ "$#" -ge 1 ]; then
		cn="$1"
		shift
	else
		cn="kids"
	fi
	[ "$#" -eq 0 ] && set -- 127.0.0.1

	san="DNS:$cn"
	for ip in "$@"; do
		san="$san,IP:$ip"
	done

	openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -days 3650 \
		-keyout "$key_file" -out "$cert_file" -subj "/CN=$cn" \
		-addext "subjectAltName=$san" 2>/dev/null

	echo "[controller.tls]"
	print_pem_block cert_pem "$cert_file"
	print_pem_block key_pem "$key_file"
	;;
client)
	cn="${1:-keycloak}"
	openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -days 3650 \
		-keyout "$key_file" -out "$cert_file" -subj "/CN=$cn" \
		-addext "basicConstraints=critical,CA:FALSE" 2>/dev/null

	echo "[[controller.tls.client_auth.clients]]"
	echo "name = \"$cn\""
	print_pem_block cert_pem "$cert_file"

	echo
	echo "# Private key for '$cn' to configure on the client side"
	cat "$key_file"
	;;
*)
	usage
	;;
esac
