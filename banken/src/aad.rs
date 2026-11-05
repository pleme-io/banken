use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

pub const AZURE_CLI_CLIENT: &str = "04b07795-8ddb-461a-bbee-02f9e1bf7b46";
pub const AUTHORITY: &str = "https://login.microsoftonline.com";
pub const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub interval: u64,
    pub expires_in: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Poll {
    Pending,
    SlowDown,
    Token(String),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub url: String,
    pub form: Vec<(String, String)>,
}

impl Request {
    #[must_use]
    pub fn shown(&self) -> String {
        let mut s = String::from("POST ");
        s.push_str(&self.url);
        for (k, v) in &self.form {
            if k == "device_code" {
                continue;
            }
            s.push_str("  ");
            s.push_str(k);
            s.push('=');
            s.push_str(v);
        }
        s
    }
}

pub trait Transport: Send + Sync {
    /// # Errors
    ///
    /// The exchange failed before an HTTP answer arrived.
    fn post_form(&self, request: &Request) -> Result<(u16, String), String>;
}

#[must_use]
pub fn scope_of(server_id: &str) -> String {
    let mut s = String::from(server_id);
    s.push_str("/.default offline_access");
    s
}

#[must_use]
pub fn device_request(tenant: &str, server_id: &str) -> Request {
    Request {
        url: format!("{AUTHORITY}/{tenant}/oauth2/v2.0/devicecode"),
        form: vec![
            ("client_id".into(), AZURE_CLI_CLIENT.into()),
            ("scope".into(), scope_of(server_id)),
        ],
    }
}

#[must_use]
pub fn token_request(tenant: &str, device_code: &str) -> Request {
    Request {
        url: format!("{AUTHORITY}/{tenant}/oauth2/v2.0/token"),
        form: vec![
            ("grant_type".into(), DEVICE_GRANT.into()),
            ("client_id".into(), AZURE_CLI_CLIENT.into()),
            ("device_code".into(), device_code.into()),
        ],
    }
}

fn field<'a>(v: &'a serde_json::Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(serde_json::Value::as_str)
}

/// # Errors
///
/// The authority refused the request or answered something that is not a
/// device code.
pub fn start(t: &dyn Transport, tenant: &str, server_id: &str) -> Result<DeviceCode, String> {
    let (status, body) = t.post_form(&device_request(tenant, server_id))?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    if status != 200 {
        return Err(field(&v, "error_description").unwrap_or(&body).to_owned());
    }
    Ok(DeviceCode {
        device_code: field(&v, "device_code").ok_or("no device_code")?.to_owned(),
        user_code: field(&v, "user_code").ok_or("no user_code")?.to_owned(),
        verification_uri: field(&v, "verification_uri")
            .ok_or("no verification_uri")?
            .to_owned(),
        interval: v
            .get("interval")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(5),
        expires_in: v
            .get("expires_in")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(900),
    })
}

#[must_use]
pub fn poll(t: &dyn Transport, tenant: &str, device_code: &str) -> Poll {
    let (status, body) = match t.post_form(&token_request(tenant, device_code)) {
        Ok(r) => r,
        Err(e) => return Poll::Failed(e),
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) else {
        return Poll::Failed(body);
    };
    if status == 200 {
        return field(&v, "access_token").map_or_else(
            || Poll::Failed("the token answer carries no access_token".into()),
            |t| Poll::Token(t.to_owned()),
        );
    }
    match field(&v, "error") {
        Some("authorization_pending") => Poll::Pending,
        Some("slow_down") => Poll::SlowDown,
        _ => Poll::Failed(field(&v, "error_description").unwrap_or(&body).to_owned()),
    }
}

fn tokens() -> &'static Mutex<HashMap<String, String>> {
    static TOKENS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    TOKENS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn remember(server_id: &str, token: String) {
    if let Ok(mut t) = tokens().lock() {
        t.insert(server_id.to_owned(), token);
    }
}

#[must_use]
pub fn token_for(server_id: &str) -> Option<String> {
    tokens().lock().ok()?.get(server_id).cloned()
}

pub struct Https;

impl Transport for Https {
    fn post_form(&self, request: &Request) -> Result<(u16, String), String> {
        let body: String = request
            .form
            .iter()
            .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        let req = http::Request::post(&request.url)
            .header("content-type", "application/x-www-form-urlencoded")
            .header("accept", "application/json")
            .body(http_body_util::Full::new(bytes::Bytes::from(body)))
            .map_err(|e| e.to_string())?;
        let fut = async move {
            let connector = hyper_rustls::HttpsConnectorBuilder::new()
                .with_native_roots()
                .map_err(|e| e.to_string())?
                .https_only()
                .enable_http1()
                .build();
            let client =
                hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
                    .build(connector);
            let res = client.request(req).await.map_err(|e| e.to_string())?;
            let status = res.status().as_u16();
            let bytes = http_body_util::BodyExt::collect(res.into_body())
                .await
                .map_err(|e| e.to_string())?
                .to_bytes();
            Ok((status, String::from_utf8_lossy(&bytes).into_owned()))
        };
        tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(fut))
    }
}

fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Recorded(Mutex<Vec<(Request, (u16, String))>>);

    impl Transport for Recorded {
        fn post_form(&self, request: &Request) -> Result<(u16, String), String> {
            let mut r = self.0.lock().unwrap();
            let (want, answer) = r.remove(0);
            assert_eq!(&want, request);
            Ok(answer)
        }
    }

    const DEVICE: &str = r#"{"user_code":"DXQ4KP7LM","device_code":"dc-1","verification_uri":"https://microsoft.com/devicelogin","expires_in":900,"interval":5,"message":"To sign in..."}"#;

    #[test]
    fn the_device_flow_asks_for_the_aks_scope_and_polls_to_a_token() {
        let t = Recorded(Mutex::new(vec![
            (device_request("organizations", "sid"), (200, DEVICE.into())),
            (
                token_request("organizations", "dc-1"),
                (400, r#"{"error":"authorization_pending"}"#.into()),
            ),
            (
                token_request("organizations", "dc-1"),
                (
                    200,
                    r#"{"access_token":"tok","token_type":"Bearer"}"#.into(),
                ),
            ),
        ]));
        let d = start(&t, "organizations", "sid").expect("device code");
        assert_eq!(d.user_code, "DXQ4KP7LM");
        assert_eq!(d.verification_uri, "https://microsoft.com/devicelogin");
        assert_eq!(poll(&t, "organizations", &d.device_code), Poll::Pending);
        assert_eq!(
            poll(&t, "organizations", &d.device_code),
            Poll::Token("tok".into())
        );
        assert!(
            device_request("t", "sid")
                .form
                .contains(&("scope".to_owned(), "sid/.default offline_access".to_owned()))
        );
    }

    #[test]
    fn a_refused_grant_is_a_failure_with_the_authority_words() {
        let t = Recorded(Mutex::new(vec![(
            token_request("t", "dc"),
            (
                400,
                r#"{"error":"expired_token","error_description":"AADSTS70020: expired"}"#.into(),
            ),
        )]));
        assert_eq!(
            poll(&t, "t", "dc"),
            Poll::Failed("AADSTS70020: expired".into())
        );
    }

    #[test]
    fn the_shown_request_never_prints_the_device_code() {
        let shown = token_request("t", "secret-dc").shown();
        assert!(!shown.contains("secret-dc"), "{shown}");
        assert!(shown.starts_with("POST https://login.microsoftonline.com/t/oauth2/v2.0/token"));
    }

    #[test]
    fn a_token_is_remembered_per_server_id() {
        remember("sid-x", "tok".into());
        assert_eq!(token_for("sid-x").as_deref(), Some("tok"));
        assert_eq!(token_for("sid-y"), None);
    }
}
