#!/usr/bin/env python3
"""Register the appliance as a client of a Keycloak realm, for sign-in with
the device-code grant (RFC 8628). Run once, as root, on the Keycloak host,
naming the live identity's admin origin, admin file and CA (#428: the
retired deployment's defaults are gone):

    sudo python3 -I config/keycloak-client.py --admin-url https://127.0.0.1:19446 --admin-file identity/admin.json --ca identity-ca.pem

It signs in to Keycloak's admin API with the administrator in --admin-file
(JSON with "username" and "password"), then creates the client or updates it
to match. The client is public (no secret): the appliance only reads who
signed in from the ID token and keeps no Keycloak token. Prints what to put
in magnus.toml. Prints no credential.

With --admin-console JWKS_FILE, it registers the admin console instead
(admin console spec): a confidential client, the code flow with PKCE, its
one redirect https://<--admin-hostname>/auth/callback, proving itself with
the console's own key, whose public half `magnus-admin jwk --config
magnus.toml` prints into JWKS_FILE; and the person's realm roles in its ID
token, since only the realm role `admin` opens the console. It also
registers the console's service account, magnus-admin-svc, proving itself
with the same key and holding this realm's management only (manage-users,
view-users, query-users, view-realm, manage-realm): the Users screen
invites, turns off and ends the sessions of the realm's people through it,
and Security › Sign-in sets the realm's sign-in rules.
"""
import argparse
import json
import ssl
import sys
from urllib.error import HTTPError
from urllib.parse import quote, urlencode
from urllib.request import Request, urlopen

p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
p.add_argument("--admin-url", required=True, help="Keycloak's private administration origin ([sign_in.oidc] status_admin_url)")
p.add_argument("--admin-file", required=True, help="JSON with the administrator's username and password (identity/admin.json beside the config)")
p.add_argument("--admin-realm", default="master")
p.add_argument("--ca", required=True, help="CA that signed the administration origin's certificate ([sign_in.oidc] status_admin_ca)")
p.add_argument("--realm", default="magnus")
p.add_argument("--client-id", default="magnus-claude")
p.add_argument("--name", default="Magnus appliance")
p.add_argument("--admin-console", metavar="JWKS_FILE", help="register the admin console's client, with the key `magnus-admin jwk` printed")
p.add_argument("--admin-hostname", action="append", default=[], help="the admin console's hostname, with :port if not 443; repeat for each name it answers to")
p.add_argument("--assign-admin", action="append", default=[], metavar="USERNAME",
               help="with --admin-console: give this person of the realm the realm role admin, which opens the console (repeatable)")
a = p.parse_args()
if a.admin_console:
    if not a.admin_hostname:
        p.error("--admin-console needs --admin-hostname")
    if a.client_id == "magnus-claude":
        a.client_id = "magnus-admin"
    if a.name == "Magnus appliance":
        a.name = "Magnus admin console"

ctx = ssl.create_default_context(cafile=a.ca)
admin = json.load(open(a.admin_file))


def send(method, url, body=None, form=None, token=None):
    headers = {}
    data = None
    if token:
        headers["Authorization"] = "Bearer " + token
    if form is not None:
        data = urlencode(form).encode()
    elif body is not None:
        headers["Content-Type"] = "application/json"
        data = json.dumps(body).encode()
    try:
        with urlopen(Request(url, data=data, method=method, headers=headers), context=ctx, timeout=15) as r:
            raw = r.read()
            return json.loads(raw) if raw else None
    except HTTPError as e:
        sys.exit(f"{method} {url}: {e.code} {e.read()[:300].decode(errors='replace')}")


token = send("POST", f"{a.admin_url}/realms/{a.admin_realm}/protocol/openid-connect/token",
             form={"client_id": "admin-cli", "grant_type": "password", "username": admin["username"], "password": admin["password"]})["access_token"]
base = f"{a.admin_url}/admin/realms/{quote(a.realm)}"
issuer = send("GET", f"{a.admin_url}/realms/{quote(a.realm)}/.well-known/openid-configuration")["issuer"]

client = {
    "clientId": a.client_id,
    "name": a.name,
    "protocol": "openid-connect",
    "enabled": True,
    "publicClient": True,
    # Only the device-code grant: no redirects, no passwords through the client.
    "standardFlowEnabled": False,
    "implicitFlowEnabled": False,
    "directAccessGrantsEnabled": False,
    "serviceAccountsEnabled": False,
    "attributes": {"oauth2.device.authorization.grant.enabled": "true"},
}
if a.admin_console:
    jwks = json.load(open(a.admin_console))
    client = {
        "clientId": a.client_id,
        "name": a.name,
        "protocol": "openid-connect",
        "enabled": True,
        "publicClient": False,
        "clientAuthenticatorType": "client-jwt",
        "standardFlowEnabled": True,
        "implicitFlowEnabled": False,
        "directAccessGrantsEnabled": False,
        "serviceAccountsEnabled": False,
        "redirectUris": [f"https://{h}/auth/callback" for h in a.admin_hostname],
        "attributes": {
            "pkce.code.challenge.method": "S256",
            "use.jwks.string": "true",
            "jwks.string": json.dumps(jwks),
            "token.endpoint.auth.signing.alg": "ES256",
        },
        "protocolMappers": [{
            "name": "realm roles", "protocol": "openid-connect", "protocolMapper": "oidc-usermodel-realm-role-mapper",
            "config": {"id.token.claim": "true", "access.token.claim": "true", "userinfo.token.claim": "true", "claim.name": "realm_access.roles", "multivalued": "true"},
        }],
    }
found = send("GET", f"{base}/clients?{urlencode({'clientId': a.client_id})}", token=token)
if found:
    send("PUT", f"{base}/clients/{found[0]['id']}", body={**found[0], **client, "attributes": {**found[0].get("attributes", {}), **client["attributes"]}}, token=token)
    print(f"Updated client {a.client_id} in realm {a.realm}.")
else:
    send("POST", f"{base}/clients", body=client, token=token)
    print(f"Created client {a.client_id} in realm {a.realm}.")

if a.admin_console:
    svc = {
        "clientId": "magnus-admin-svc",
        "name": "Magnus admin console (people)",
        "protocol": "openid-connect",
        "enabled": True,
        "publicClient": False,
        "clientAuthenticatorType": "client-jwt",
        "standardFlowEnabled": False,
        "implicitFlowEnabled": False,
        "directAccessGrantsEnabled": False,
        "serviceAccountsEnabled": True,
        "attributes": {"use.jwks.string": "true", "jwks.string": json.dumps(jwks), "token.endpoint.auth.signing.alg": "ES256"},
    }
    found = send("GET", f"{base}/clients?{urlencode({'clientId': svc['clientId']})}", token=token)
    if found:
        send("PUT", f"{base}/clients/{found[0]['id']}", body={**found[0], **svc, "attributes": {**found[0].get("attributes", {}), **svc["attributes"]}}, token=token)
    else:
        send("POST", f"{base}/clients", body=svc, token=token)
    cid = send("GET", f"{base}/clients?{urlencode({'clientId': svc['clientId']})}", token=token)[0]["id"]
    account = send("GET", f"{base}/clients/{cid}/service-account-user", token=token)["id"]
    rm = send("GET", f"{base}/clients?{urlencode({'clientId': 'realm-management'})}", token=token)[0]["id"]
    roles = [send("GET", f"{base}/clients/{rm}/roles/{r}", token=token) for r in ("manage-users", "view-users", "query-users", "view-realm", "manage-realm")]
    send("POST", f"{base}/users/{account}/role-mappings/clients/{rm}", body=roles, token=token)
    print(f"The console's service account magnus-admin-svc may manage the people and the sign-in rules of realm {a.realm}, and no more.")
    # The realm role that opens the console, and the people who hold it.
    if a.assign_admin:
        try:
            role = send("GET", f"{base}/roles/admin", token=token)
        except SystemExit:
            send("POST", f"{base}/roles", body={"name": "admin", "description": "Opens the Magnus admin console"}, token=token)
            role = send("GET", f"{base}/roles/admin", token=token)
        for name in a.assign_admin:
            users = [u for u in send("GET", f"{base}/users?{urlencode({'username': name, 'exact': 'true'})}", token=token) if u.get("username") == name]
            if not users:
                print(f"No person {name} in realm {a.realm}: not given the role admin.")
                continue
            send("POST", f"{base}/users/{users[0]['id']}/role-mappings/realm", body=[role], token=token)
            print(f"{name} holds the realm role admin.")
    print(f"""
The admin console signs in through {issuer} as {a.client_id}. Give the
realm role "admin" to the people who run the appliance; only they enter.
""")
    sys.exit(0)

print(f"""
Add to magnus.toml, then restart the appliance:

[sign_in.oidc]
issuer = "{issuer}"
client_id = "{a.client_id}"
""")
