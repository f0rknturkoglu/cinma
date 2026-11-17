//! System-browser OAuth PKCE / Authorization Code helpers for Simkl, AniList & MAL.
//! Never uses an embedded WebView (`CLAUDE.md` §1.2).

use ring::rand::{SecureRandom, SystemRandom};
use serde::Deserialize;
use url::{Url, form_urlencoded};

use crate::{TrackingClient, TrackingError};

pub const OAUTH_REDIRECT_URI: &str = "cinma://oauth/callback";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuthProvider {
    Simkl,
    AniList,
    Mal,
}

impl OAuthProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Simkl => "simkl",
            Self::AniList => "anilist",
            Self::Mal => "mal",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "simkl" => Some(Self::Simkl),
            "anilist" => Some(Self::AniList),
            "mal" | "myanimelist" => Some(Self::Mal),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingOAuthFlow {
    pub provider: OAuthProvider,
    pub authorize_url: String,
    pub state: String,
    pub code_verifier: String,
    pub client_id: String,
    pub client_secret: String,
}

fn random_urlsafe(len_bytes: usize) -> Result<String, TrackingError> {
    let rng = SystemRandom::new();
    let mut bytes = vec![0u8; len_bytes];
    rng.fill(&mut bytes)
        .map_err(|_| TrackingError::Invalid("CSPRNG failure".into()))?;
    Ok(base64url(&bytes))
}

fn base64url(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = *chunk.get(1).unwrap_or(&0) as usize;
        let b2 = *chunk.get(2).unwrap_or(&0) as usize;
        out.push(TABLE[b0 >> 2] as char);
        out.push(TABLE[((b0 & 0x03) << 4) | (b1 >> 4)] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((b1 & 0x0F) << 2) | (b2 >> 6)] as char);
        }
        if chunk.len() > 2 {
            out.push(TABLE[b2 & 0x3F] as char);
        }
    }
    out
}

pub fn build_oauth_request(
    provider: OAuthProvider,
    client_id: &str,
    client_secret: &str,
) -> Result<PendingOAuthFlow, TrackingError> {
    let cid = client_id.trim();
    if cid.is_empty() {
        return Err(TrackingError::Invalid(format!(
            "{} client_id cannot be empty",
            provider.as_str()
        )));
    }
    let state = format!("{}:{}", provider.as_str(), random_urlsafe(18)?);
    // MAL accepts PKCE `plain` method reliably with 43..128 unreserved chars.
    let verifier = random_urlsafe(48)?;
    let redirect = format!("cinma://oauth/{}", provider.as_str());

    let mut url = match provider {
        OAuthProvider::Simkl => Url::parse("https://simkl.com/oauth/authorize"),
        OAuthProvider::AniList => Url::parse("https://anilist.co/api/v2/oauth/authorize"),
        OAuthProvider::Mal => Url::parse("https://myanimelist.net/v1/oauth2/authorize"),
    }
    .map_err(|e| TrackingError::Invalid(e.to_string()))?;

    {
        let mut q = url.query_pairs_mut();
        q.append_pair("response_type", "code");
        q.append_pair("client_id", cid);
        q.append_pair("redirect_uri", &redirect);
        q.append_pair("state", &state);
        if provider == OAuthProvider::Mal {
            q.append_pair("code_challenge", &verifier);
            q.append_pair("code_challenge_method", "plain");
        }
    }

    Ok(PendingOAuthFlow {
        provider,
        authorize_url: url.into(),
        state,
        code_verifier: verifier,
        client_id: cid.to_owned(),
        client_secret: client_secret.trim().to_owned(),
    })
}

pub fn parse_oauth_callback(
    callback_url: &str,
) -> Result<(OAuthProvider, String, String), TrackingError> {
    let url = Url::parse(callback_url).map_err(|e| TrackingError::Invalid(e.to_string()))?;
    let host = url.host_str().unwrap_or("");
    let path_seg = url.path().trim_matches('/');
    let provider_hint = if host == "oauth" && !path_seg.is_empty() {
        path_seg
    } else {
        host
    };
    let mut code = None;
    let mut state = None;
    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "code" => code = Some(v.into_owned()),
            "state" => state = Some(v.into_owned()),
            "error" => {
                return Err(TrackingError::Invalid(format!("OAuth error: {v}")));
            }
            _ => {}
        }
    }
    let state = state.ok_or_else(|| TrackingError::Invalid("missing OAuth state".into()))?;
    let provider = OAuthProvider::parse(provider_hint)
        .or_else(|| state.split(':').next().and_then(OAuthProvider::parse))
        .ok_or_else(|| TrackingError::Invalid("unknown OAuth provider in callback".into()))?;
    let code =
        code.ok_or_else(|| TrackingError::Invalid("missing OAuth authorization code".into()))?;
    Ok((provider, code, state))
}

#[derive(Deserialize)]
struct OAuthTokenResp {
    access_token: String,
}

impl TrackingClient {
    pub async fn exchange_oauth_code(
        &self,
        flow: &PendingOAuthFlow,
        code: &str,
        token_url_override: Option<&str>,
    ) -> Result<String, TrackingError> {
        let redirect = format!("cinma://oauth/{}", flow.provider.as_str());
        let resp = match flow.provider {
            OAuthProvider::Simkl => {
                let url = token_url_override
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("{}/oauth/token", self.simkl_base));
                let body = serde_json::json!({
                    "code": code,
                    "client_id": flow.client_id,
                    "client_secret": flow.client_secret,
                    "redirect_uri": redirect,
                    "grant_type": "authorization_code"
                });
                self.http
                    .post(url)
                    .header("Content-Type", "application/json")
                    .body(serde_json::to_vec(&body)?)
                    .send()
                    .await?
            }
            OAuthProvider::AniList => {
                let url = token_url_override.unwrap_or("https://anilist.co/api/v2/oauth/token");
                let body = serde_json::json!({
                    "grant_type": "authorization_code",
                    "client_id": flow.client_id,
                    "client_secret": flow.client_secret,
                    "redirect_uri": redirect,
                    "code": code
                });
                self.http
                    .post(url)
                    .header("Content-Type", "application/json")
                    .header("Accept", "application/json")
                    .body(serde_json::to_vec(&body)?)
                    .send()
                    .await?
            }
            OAuthProvider::Mal => {
                let url = token_url_override.unwrap_or("https://myanimelist.net/v1/oauth2/token");
                let form_body = {
                    let mut ser = form_urlencoded::Serializer::new(String::new());
                    ser.append_pair("client_id", &flow.client_id);
                    if !flow.client_secret.is_empty() {
                        ser.append_pair("client_secret", &flow.client_secret);
                    }
                    ser.append_pair("code", code);
                    ser.append_pair("code_verifier", &flow.code_verifier);
                    ser.append_pair("grant_type", "authorization_code");
                    ser.append_pair("redirect_uri", &redirect);
                    ser.finish()
                };
                self.http
                    .post(url)
                    .header("Content-Type", "application/x-www-form-urlencoded")
                    .body(form_body)
                    .send()
                    .await?
            }
        };
        let bytes = crate::check_status("OAuth", resp).await?;
        let parsed: OAuthTokenResp = serde_json::from_slice(&bytes)?;
        if parsed.access_token.trim().is_empty() {
            return Err(TrackingError::Invalid(
                "OAuth token response was empty".into(),
            ));
        }
        Ok(parsed.access_token)
    }
}
