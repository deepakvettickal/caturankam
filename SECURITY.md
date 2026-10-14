# Security

## Reporting a vulnerability

Please report security issues privately through
[GitHub's private vulnerability reporting](https://github.com/deepakvettickal/caturankam/security/advisories/new),
not in a public issue. You'll get a reply within a week.

## What Caturankam does with your account

- It logs in with OAuth 2 (Authorization Code + PKCE) in your own browser, so the app
  never sees your password. It asks only for the `board:play` and `preference:read`
  permissions.
- The access token is stored in the macOS Keychain, never in a file, and is revoked
  on Lichess when you log out.
- It talks only to `lichess.org` and `lichess1.org` (piece sets and boards), over TLS.

Only the latest release gets security fixes.
