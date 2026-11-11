use reqwest::blocking::Client;

fn konst(name: &str) -> String {
    let src = include_str!("../src/yandex.rs");
    let pat = format!("const {}: &str = \"", name);
    let start = src.find(&pat).unwrap() + pat.len();
    src[start..].split('"').next().unwrap().to_string()
}

const BROWSER_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";

#[test]
fn diag_yandex_variants() {
    let refresh = std::env::var("YANDEX_REFRESH_TOKEN").unwrap();
    let json: serde_json::Value = Client::new()
        .post("https://oauth.yandex.ru/token")
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh.as_str()),
            ("client_id", konst("CLIENT_ID").as_str()),
            ("client_secret", konst("CLIENT_SECRET").as_str()),
        ])
        .send()
        .unwrap()
        .json()
        .unwrap();
    let access = json["access_token"].as_str().unwrap().to_string();
    let url = "https://api.contest.yandex.net/api/public/v2/compilers";
    let variants: Vec<(&str, bool, Option<&str>)> = vec![
        ("native-tls, no UA", false, None),
        ("native-tls, browser UA", false, Some(BROWSER_UA)),
        ("native-tls, curl UA", false, Some("curl/8.5.0")),
        ("native-tls, submitter UA", false, Some("submitter/0.1.0")),
        ("rustls, no UA", true, None),
        ("rustls, browser UA", true, Some(BROWSER_UA)),
        ("native-tls, no UA (again)", false, None),
    ];
    for (label, rustls, ua) in variants {
        let mut b = Client::builder().redirect(reqwest::redirect::Policy::none());
        if rustls { b = b.use_rustls_tls(); }
        let client = b.build().unwrap();
        let mut req = client.get(url).header("Authorization", format!("OAuth {}", access));
        if let Some(ua) = ua {
            req = req.header("User-Agent", ua);
        }
        match req.send() {
            Ok(resp) => println!(
                "VARIANT {:28} -> {} {:?} captcha={:?}",
                label,
                resp.status(),
                resp.version(),
                resp.headers().get("x-yandex-captcha")
            ),
            Err(e) => println!("VARIANT {:28} -> ERROR {}", label, e),
        }
    }
}
