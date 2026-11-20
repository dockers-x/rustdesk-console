#!/usr/bin/env bash
set -euo pipefail

# Point these paths at an unpacked OpenLDAP build when it is not installed.
slapd="${LDAP_TEST_BIN_DIR:+$LDAP_TEST_BIN_DIR/}slapd"
slapadd="${LDAP_TEST_BIN_DIR:+$LDAP_TEST_BIN_DIR/}slapadd"
ldapwhoami="${LDAP_TEST_BIN_DIR:+$LDAP_TEST_BIN_DIR/}ldapwhoami"
schema="${LDAP_TEST_SCHEMA_DIR:-/etc/openldap/schema}"

fixture="$(mktemp -d)"
cleanup() {
    if [[ -n "${server_pid:-}" ]]; then
        kill "$server_pid" 2>/dev/null || true
        wait "$server_pid" 2>/dev/null || true
    fi
    rm -rf "$fixture"
}
trap cleanup EXIT

mkdir "$fixture/db"
openssl req -x509 -newkey rsa:2048 -nodes -days 1 -subj '/CN=LDAP test CA' \
    -keyout "$fixture/ca.key" -out "$fixture/ca.pem" 2>/dev/null
openssl req -new -newkey rsa:2048 -nodes -subj '/CN=127.0.0.1' \
    -keyout "$fixture/server.key" -out "$fixture/server.csr" 2>/dev/null
cat > "$fixture/extensions" <<'EXTENSIONS'
subjectAltName = IP:127.0.0.1
extendedKeyUsage = serverAuth
EXTENSIONS
openssl x509 -req -days 1 -in "$fixture/server.csr" -CA "$fixture/ca.pem" \
    -CAkey "$fixture/ca.key" -extfile "$fixture/extensions" \
    -out "$fixture/server.pem" 2>/dev/null

cat > "$fixture/slapd.conf" <<CONF
include $schema/core.schema
include $schema/cosine.schema
include $schema/inetorgperson.schema
pidfile $fixture/slapd.pid
argsfile $fixture/slapd.args
TLSCACertificateFile $fixture/ca.pem
TLSCertificateFile $fixture/server.pem
TLSCertificateKeyFile $fixture/server.key
database mdb
suffix "dc=example,dc=test"
rootdn "cn=fixture,dc=example,dc=test"
rootpw fixture-service-password
directory $fixture/db
CONF
cat > "$fixture/data.ldif" <<'LDIF'
dn: dc=example,dc=test
objectClass: top
objectClass: dcObject
objectClass: organization
dc: example
o: LDAP Test

dn: ou=users,dc=example,dc=test
objectClass: organizationalUnit
ou: users

dn: uid=directory-user,ou=users,dc=example,dc=test
objectClass: inetOrgPerson
uid: directory-user
cn: Directory User
sn: User
givenName: Directory
mail: directory-user@example.test
userPassword: fixture-user-password

dn: uid=second-user,ou=users,dc=example,dc=test
objectClass: inetOrgPerson
uid: second-user
cn: Second User
sn: User
userPassword: fixture-user-password
LDIF
"$slapadd" -f "$fixture/slapd.conf" -l "$fixture/data.ldif"
read -r plain secure < <(python3 - <<'PY'
from socket import socket
with socket() as plain, socket() as secure:
    plain.bind(("127.0.0.1", 0))
    secure.bind(("127.0.0.1", 0))
    print(plain.getsockname()[1], secure.getsockname()[1])
PY
)
export LDAP_TEST_PLAIN="ldap://127.0.0.1:$plain"
export LDAP_TEST_SECURE="ldaps://127.0.0.1:$secure"
export LDAP_TEST_CA="$fixture/ca.pem"
"$slapd" -d 0 -f "$fixture/slapd.conf" -h "$LDAP_TEST_PLAIN $LDAP_TEST_SECURE" > "$fixture/slapd.log" 2>&1 &
server_pid=$!
for attempt in {1..30}; do
    if "$ldapwhoami" -x -H "$LDAP_TEST_PLAIN" -o nettimeout=1 >/dev/null 2>&1; then
        cargo test -p rustdesk-console services::ldap::tests::ldap_openldap_loopback -- --ignored --exact 2>&1 | tee "$fixture/test.log"
        # The harness filters by full module name when --exact is specified.
        grep -F '1 passed; 0 failed' "$fixture/test.log"
        exit
    fi
    sleep 0.1
done
cat "$fixture/slapd.log" >&2
exit 1
