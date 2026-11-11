use submitter::yandex::YandexClient;

// Read-only: the same first steps as test_yandex, without submitting.
#[test]
fn diag_yandex_compilers() {
    let refresh = std::env::var("YANDEX_REFRESH_TOKEN").unwrap();
    let mut client = YandexClient::new();
    client.set_refresh_token(&refresh);
    assert!(client.try_refresh_token(), "refresh failed");
    for lang in ["C++23", "Rust"] {
        let (id, name) = client.find_compiler(lang).unwrap();
        println!("VARIANT find_compiler({}) -> {} / {}", lang, id, name);
    }
    for path in ["/service/introspect", "/contests/90118"] {
        let resp = client.http_mut().get(path).unwrap();
        println!("VARIANT {} -> {} {:?}", path, resp.status(), resp.version());
        assert!(resp.status().is_success());
    }
}
