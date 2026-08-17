# Security policy

Please report suspected vulnerabilities privately through GitHub's security-advisory feature. Do
not include live API keys, OAuth tokens, UCloud private keys, or user configuration in a public
issue.

## UCloud OpenAPI SHA-1 compatibility boundary

`src/ucloud.rs::create_signature` intentionally implements the UCloud OpenAPI V1 wire protocol:
sort all request parameters, concatenate each key and value, append the `PrivateKey`, and compute a
SHA-1 digest. The server requires this exact algorithm; replacing it locally with HMAC-SHA256 would
make project discovery, API-key creation, and usage queries fail authentication.

This is a narrow protocol exception, not a general-purpose cryptographic choice:

- the function is used only for UCloud OpenAPI request signing;
- production requests use the UCloud HTTPS endpoint;
- the private key is appended locally and is never sent as a request parameter;
- no new AstraFlow protocol may use SHA-1;
- migration requires a server-supported modern signing version and coordinated client rollout.

The inline Semgrep suppression applies only to this mandated digest operation. All release-artifact
integrity checks use SHA-256.
