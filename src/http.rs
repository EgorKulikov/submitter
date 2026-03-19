use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, COOKIE, SET_COOKIE};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::time::Duration;

const COOKIE_FILE: &str = ".submitter_cookies.json";

/// Global cookie store: domain -> {name -> value}
#[derive(Serialize, Deserialize, Default)]
struct CookieStore {
    domains: HashMap<String, HashMap<String, String>>,
}

impl CookieStore {
    fn load() -> Self {
        fs::read_to_string(COOKIE_FILE)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save(&self) {
        match serde_json::to_string_pretty(self) {
            Ok(s) => {
                if let Err(e) = fs::write(COOKIE_FILE, s) {
                    eprintln!(
                        "Warning: failed to save cookies to {}: {}",
                        COOKIE_FILE, e
                    );
                }
            }
            Err(e) => {
                eprintln!("Warning: failed to serialize cookies: {}", e);
            }
        }
    }
}

pub struct HttpClient {
    client: Client,
    base_url: String,
    domain: String,
    extra_headers: HeaderMap,
    send_cookies: bool,
}

impl HttpClient {
    pub fn new(base_url: &str) -> Self {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap();
        let domain = base_url
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap_or("unknown")
            .to_string();
        HttpClient {
            client,
            base_url: base_url.to_string(),
            domain,
            extra_headers: HeaderMap::new(),
            send_cookies: true,
        }
    }

    pub fn cookie_header(&self) -> String {
        let store = CookieStore::load();
        store
            .domains
            .get(&self.domain)
            .map(|cookies| {
                cookies
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>()
                    .join("; ")
            })
            .unwrap_or_default()
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn set_header(&mut self, name: &str, value: &str) {
        if let (Ok(n), Ok(v)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(value),
        ) {
            self.extra_headers.insert(n, v);
        }
    }

    pub fn disable_cookie_sending(&mut self) {
        self.send_cookies = false;
    }

    pub fn clear_cookies(&mut self) {
        let mut store = CookieStore::load();
        store.domains.remove(&self.domain);
        store.save();
    }

    pub fn get_cookie(&self, name: &str) -> Option<String> {
        let store = CookieStore::load();
        store
            .domains
            .get(&self.domain)?
            .get(name)
            .cloned()
    }

    pub fn set_cookie(&mut self, name: &str, value: &str) {
        let mut store = CookieStore::load();
        store
            .domains
            .entry(self.domain.clone())
            .or_default()
            .insert(name.to_string(), value.to_string());
        store.save();
    }

    fn save_response_cookies(&self, resp: &reqwest::blocking::Response) {
        let mut any = false;
        let mut store = CookieStore::load();
        let cookies = store
            .domains
            .entry(self.domain.clone())
            .or_default();
        for val in resp.headers().get_all(SET_COOKIE) {
            if let Ok(s) = val.to_str() {
                if let Some(nv) = s.split(';').next() {
                    if let Some(eq) = nv.find('=') {
                        let name = nv[..eq].trim().to_string();
                        let value = nv[eq + 1..].trim().to_string();
                        cookies.insert(name, value);
                        any = true;
                    }
                }
            }
        }
        if any {
            store.save();
        }
    }

    fn resolve_url(&self, url_or_path: &str) -> String {
        if url_or_path.starts_with("http://") || url_or_path.starts_with("https://") {
            url_or_path.to_string()
        } else {
            format!("{}{}", self.base_url, url_or_path)
        }
    }

    fn apply_cookies(
        &self,
        req: reqwest::blocking::RequestBuilder,
    ) -> reqwest::blocking::RequestBuilder {
        if !self.send_cookies {
            return req;
        }
        let cookie_header = self.cookie_header();
        if cookie_header.is_empty() {
            req
        } else {
            req.header(COOKIE, &cookie_header)
        }
    }

    fn apply_headers(
        &self,
        mut req: reqwest::blocking::RequestBuilder,
    ) -> reqwest::blocking::RequestBuilder {
        for (name, value) in &self.extra_headers {
            req = req.header(name.clone(), value.clone());
        }
        req
    }

    fn follow_redirect(
        &self,
        resp: reqwest::blocking::Response,
    ) -> Result<reqwest::blocking::Response, String> {
        if resp.status().is_redirection() {
            if let Some(loc) = resp.headers().get("location") {
                if let Ok(loc_str) = loc.to_str() {
                    let redirect_url = self.resolve_url(loc_str);
                    let req = self.client.get(&redirect_url);
                    let req = self.apply_cookies(req);
                    let req = self.apply_headers(req);
                    let resp2 = req
                        .send()
                        .map_err(|e| format!("Redirect to {} failed: {}", redirect_url, e))?;
                    self.save_response_cookies(&resp2);
                    return Ok(resp2);
                }
            }
        }
        Ok(resp)
    }

    pub fn get(&mut self, path: &str) -> Result<reqwest::blocking::Response, String> {
        let url = self.resolve_url(path);
        let req = self.client.get(&url);
        let req = self.apply_cookies(req);
        let req = self.apply_headers(req);
        let resp = req
            .send()
            .map_err(|e| format!("GET {} failed: {}", url, e))?;
        self.save_response_cookies(&resp);
        self.follow_redirect(resp)
    }

    pub fn get_with_header(
        &mut self,
        path: &str,
        header_name: &str,
        header_value: &str,
    ) -> Result<reqwest::blocking::Response, String> {
        let url = self.resolve_url(path);
        let req = self.client.get(&url);
        let req = self.apply_cookies(req);
        let req = self.apply_headers(req);
        let resp = req
            .header(header_name, header_value)
            .send()
            .map_err(|e| format!("GET {} failed: {}", url, e))?;
        self.save_response_cookies(&resp);
        self.follow_redirect(resp)
    }

    pub fn post_multipart(
        &mut self,
        path: &str,
        form: reqwest::blocking::multipart::Form,
        auth: &str,
    ) -> Result<reqwest::blocking::Response, String> {
        let url = self.resolve_url(path);
        let req = self.client.post(&url);
        let req = self.apply_cookies(req);
        let req = self.apply_headers(req);
        let resp = req
            .header("Authorization", auth)
            .multipart(form)
            .send()
            .map_err(|e| format!("POST {} failed: {}", url, e))?;
        self.save_response_cookies(&resp);
        self.follow_redirect(resp)
    }

    pub fn post_form(
        &mut self,
        path: &str,
        form: &[(&str, &str)],
    ) -> Result<reqwest::blocking::Response, String> {
        let url = self.resolve_url(path);
        let req = self.client.post(&url);
        let req = self.apply_cookies(req);
        let req = self.apply_headers(req);
        let resp = req
            .form(form)
            .send()
            .map_err(|e| format!("POST {} failed: {}", url, e))?;
        self.save_response_cookies(&resp);
        self.follow_redirect(resp)
    }

    pub fn post_json(
        &mut self,
        path: &str,
        json_body: &str,
    ) -> Result<reqwest::blocking::Response, String> {
        let url = self.resolve_url(path);
        let req = self.client.post(&url);
        let req = self.apply_cookies(req);
        let req = self.apply_headers(req);
        let resp = req
            .header("Content-Type", "application/json")
            .body(json_body.to_string())
            .send()
            .map_err(|e| format!("POST {} failed: {}", url, e))?;
        self.save_response_cookies(&resp);
        self.follow_redirect(resp)
    }

    pub fn get_text(&mut self, path: &str) -> Result<String, String> {
        let resp = self.get(path)?;
        resp.text()
            .map_err(|e| format!("Failed to read response: {}", e))
    }

    pub fn get_json(&mut self, path: &str) -> Result<serde_json::Value, String> {
        let body = self.get_text(path)?;
        serde_json::from_str(&body).map_err(|e| {
            format!(
                "Failed to parse JSON: {} body: {}",
                e,
                &body[..body.len().min(200)]
            )
        })
    }

    pub fn post_form_json(
        &mut self,
        path: &str,
        form: &[(&str, &str)],
    ) -> Result<serde_json::Value, String> {
        let resp = self.post_form(path, form)?;
        let body = resp
            .text()
            .map_err(|e| format!("Failed to read response: {}", e))?;
        serde_json::from_str(&body).map_err(|e| {
            format!(
                "Failed to parse JSON: {} body: {}",
                e,
                &body[..body.len().min(200)]
            )
        })
    }
}
