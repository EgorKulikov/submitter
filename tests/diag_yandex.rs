use submitter::yandex::YandexClient;

#[test]
fn diag_yandex_compilers() {
    let refresh = std::env::var("YANDEX_REFRESH_TOKEN").unwrap();
    let mut client = YandexClient::new();
    client.set_refresh_token(&refresh);
    assert!(client.try_refresh_token(), "refresh failed");
    for path in ["/compilers", "/service/introspect", "/contests/90118"] {
        let resp = client.http_mut().get(path).unwrap();
        println!("=== {} -> {} {:?}", path, resp.status(), resp.version());
        for (k, v) in resp.headers() {
            if k.as_str() != "set-cookie" {
                println!("{}: {:?}", k, v);
            }
        }
        let body = resp.text().unwrap();
        println!("--- body bytes: {}", body.len());
        if !body.starts_with('{') && !body.starts_with('[') {
            let mut text = String::new();
            let mut in_tag = false;
            for ch in body.chars() {
                match ch {
                    '<' => in_tag = true,
                    '>' => { in_tag = false; text.push(' '); }
                    c if !in_tag => text.push(c),
                    _ => {}
                }
            }
            let text: Vec<&str> = text.split_whitespace().collect();
            let text = text.join(" ");
            println!("{}", text.chars().take(3000).collect::<String>());
        }
    }
}
