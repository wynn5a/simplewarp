use std::collections::HashMap;

use anyhow::Result;
use url::Url;
use warp_core::safe_anyhow;

use super::UserUid;
use super::credentials::RefreshToken;

const AUTH_URL_HOST: &str = "auth";
const AUTH_URL_REFRESH_TOKEN_QUERY_PARAM: &str = "refresh_token";
const AUTH_URL_NEW_USER_UID_QUERY_PARAM: &str = "user_uid";
const AUTH_URL_DELETED_ANON_USER_QUERY_PARAM: &str = "deleted_anonymous_user";
const AUTH_URL_STATE_QUERY_PARAM: &str = "state";

// `AuthRedirectPayload` is returned from the incoming redirect url.
#[derive(Debug, Clone)]
pub struct AuthRedirectPayload {
    pub refresh_token: RefreshToken,
    pub user_uid: Option<UserUid>,
    pub deleted_anonymous_user: Option<bool>,
    pub state: Option<String>,
}

impl AuthRedirectPayload {
    /// Attempts to parse the `AuthRedirectPayload` from URL sent to Warp. To parse successfully, the URL
    /// must be of format {scheme}://auth/desktop_redirect?refresh_token={token}.
    pub fn from_url(url: Url) -> Result<Self> {
        if url.host_str() != Some(AUTH_URL_HOST) {
            return Err(safe_anyhow!(
                safe: ("Auth redirect URL has unexpected host"),
                full: ("Received URL with unexpected host: {} ", url)
            ));
        }
        let query_params: HashMap<_, _> = url.query_pairs().into_owned().collect();
        if let Some(token) = query_params.get(AUTH_URL_REFRESH_TOKEN_QUERY_PARAM) {
            let user_uid = query_params
                .get(AUTH_URL_NEW_USER_UID_QUERY_PARAM)
                .map(|uid| UserUid::new(uid));

            Ok(Self {
                refresh_token: RefreshToken::new(token),
                user_uid,
                deleted_anonymous_user: query_params
                    .get(AUTH_URL_DELETED_ANON_USER_QUERY_PARAM)
                    .map(|value| value == "true"),
                state: query_params.get(AUTH_URL_STATE_QUERY_PARAM).cloned(),
            })
        } else {
            Err(safe_anyhow!(
                safe: ("Auth redirect URL is missing required credential"),
                full: ("Received URL without refresh token query param: {}", url)
            ))
        }
    }
}
