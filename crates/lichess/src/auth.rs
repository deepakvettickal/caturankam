//! OAuth2 Authorization Code flow with PKCE and a loopback redirect.
//! Lichess needs no app registration: `client_id` is any identifier.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::{Error, LICHESS};

pub const CLIENT_ID: &str = "caturankam";
pub const SCOPES: &str = "board:play preference:read";

/// How long to wait for the user to finish in the browser.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);

/// Runs the whole flow and returns an access token. `open_browser` is called
/// with the authorization URL; the library stays free of platform code.
pub async fn login(
    http: &reqwest::Client,
    open_browser: impl FnOnce(&str),
) -> Result<String, Error> {
    let verifier = random_token()?;
    let state = random_token()?;
    // Port 0: the OS picks a free port. Loopback only, never reachable from the network.
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let redirect_uri = format!(
        "http://127.0.0.1:{}/callback",
        listener.local_addr()?.port()
    );

    let url = reqwest::Url::parse_with_params(
        &format!("{LICHESS}/oauth"),
        [
            ("response_type", "code"),
            ("client_id", CLIENT_ID),
            ("redirect_uri", &redirect_uri),
            ("code_challenge_method", "S256"),
            ("code_challenge", &challenge(&verifier)),
            ("scope", SCOPES),
            ("state", &state),
        ],
    )
    .map_err(|e| Error::Auth(e.to_string()))?;
    open_browser(url.as_str());

    let code = tokio::time::timeout(LOGIN_TIMEOUT, wait_for_code(&listener, &state))
        .await
        .map_err(|_| Error::Auth("timed out waiting for the browser".into()))??;

    #[derive(Deserialize)]
    struct Token {
        access_token: String,
    }
    let token: Token = crate::json(http.post(format!("{LICHESS}/api/token")).form(&[
        ("grant_type", "authorization_code"),
        ("code", &code),
        ("code_verifier", &verifier),
        ("redirect_uri", &redirect_uri),
        ("client_id", CLIENT_ID),
    ]))
    .await?;
    Ok(token.access_token)
}

/// Accepts loopback connections until one carries our callback. Stray
/// requests (favicon, port scanners) are answered with 404 and ignored.
async fn wait_for_code(listener: &TcpListener, state: &str) -> Result<String, Error> {
    loop {
        let (mut sock, _) = listener.accept().await?;
        let mut buf = vec![0; 4096];
        let n = sock.read(&mut buf).await?;
        let request = String::from_utf8_lossy(&buf[..n]);
        let Some(result) = parse_callback(&request, state) else {
            let _ = sock
                .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n")
                .await;
            continue;
        };
        let body = match &result {
            Ok(_) => "Logged in. You can close this tab and return to Caturankam.",
            Err(_) => "Login failed. You can close this tab and return to Caturankam.",
        };
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = sock.write_all(response.as_bytes()).await;
        return result;
    }
}

/// Parses `GET /callback?code=..&state=.. HTTP/1.1`. `None` if it isn't our callback.
fn parse_callback(request: &str, expected_state: &str) -> Option<Result<String, Error>> {
    let target = request
        .lines()
        .next()?
        .strip_prefix("GET ")?
        .split(' ')
        .next()?;
    let url = reqwest::Url::parse(&format!("http://127.0.0.1{target}")).ok()?;
    if url.path() != "/callback" {
        return None;
    }
    let param = |k: &str| {
        url.query_pairs()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.into_owned())
    };
    // CSRF check first: a callback with the wrong state is never trusted, not even its error.
    if param("state").as_deref() != Some(expected_state) {
        return Some(Err(Error::Auth("state mismatch".into())));
    }
    if let Some(err) = param("error") {
        return Some(Err(Error::Auth(err)));
    }
    Some(param("code").ok_or_else(|| Error::Auth("no code in callback".into())))
}

/// 32 random bytes, base64url: 43 chars, within PKCE's 43..=128 range.
fn random_token() -> Result<String, Error> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| Error::Auth(e.to_string()))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_rfc7636_vector() {
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let t = random_token().unwrap();
        assert_eq!(t.len(), 43);
        assert_ne!(t, random_token().unwrap());
    }

    #[test]
    fn callback_parsing() {
        let get = |q: &str| format!("GET {q} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
        let ok = parse_callback(&get("/callback?code=liu_abc&state=s1"), "s1");
        assert_eq!(ok.unwrap().unwrap(), "liu_abc");

        let csrf = parse_callback(&get("/callback?code=liu_abc&state=evil"), "s1");
        assert!(matches!(csrf, Some(Err(Error::Auth(m))) if m == "state mismatch"));

        let denied = parse_callback(&get("/callback?error=access_denied&state=s1"), "s1");
        assert!(matches!(denied, Some(Err(Error::Auth(m))) if m == "access_denied"));

        let no_state = parse_callback(&get("/callback?error=access_denied"), "s1");
        assert!(matches!(no_state, Some(Err(Error::Auth(m))) if m == "state mismatch"));

        assert!(parse_callback(&get("/favicon.ico"), "s1").is_none());
        assert!(parse_callback("garbage", "s1").is_none());
    }
}
