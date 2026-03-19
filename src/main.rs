use regex::Regex;
use std::env;
use std::fs::read_to_string;
use submitter::{codechef, codeforces, eolymp, kattis, toph, ucup, yandex};

fn site_key_from_url(url: &str) -> Option<String> {
    let url_regex = Regex::new(r"https?://(?:www\.)?([^/]+).*").unwrap();
    let domain = url_regex.captures(url)?[1].to_string();
    let domain_parts: Vec<&str> = domain.split('.').collect();
    if domain_parts.len() >= 2 {
        Some(format!(
            "{}.{}",
            domain_parts[domain_parts.len() - 2],
            domain_parts[domain_parts.len() - 1]
        ))
    } else {
        Some(domain)
    }
}

fn short_name_to_site_key(name: &str) -> Option<String> {
    let key = match name.to_lowercase().as_str() {
        "cf" | "codeforces" => "codeforces.com",
        "cc" | "codechef" => "codechef.com",
        "ucup" => "ucup.ac",
        "uoj" => "uoj.ac",
        "qoj" => "qoj.ac",
        "yandex" | "ya" => "yandex.ru",
        "toph" => "toph.co",
        "kattis" => "kattis.com",
        "eolymp" | "eol" => "eolymp.com",
        _ => return None,
    };
    Some(key.to_string())
}

fn do_login(site_key: &str) {
    match site_key {
        "codeforces.com" => codeforces::login(),
        "ucup.ac" => ucup::login(),
        "uoj.ac" | "qoj.ac" => {
            let (base, name) = if site_key == "qoj.ac" {
                ("https://qoj.ac", "QOJ")
            } else {
                ("https://uoj.ac", "UOJ")
            };
            let mut client = submitter::uoj::UojClient::new(base, name);
            if let Err(e) = client.login() {
                eprintln!("Login failed: {}", e);
            }
        }
        "yandex.com" | "yandex.ru" => yandex::login(),
        "codechef.com" => codechef::login(),
        "toph.co" => toph::login(),
        "kattis.com" => kattis::login(),
        "eolymp.com" => eolymp::login(),
        _ => eprintln!("Unsupported site: {}", site_key),
    }
}

fn main() {
    let args: Vec<_> = env::args().collect();

    if args.len() == 3 && args[1] == "login" {
        let site = &args[2];
        let key = short_name_to_site_key(site)
            .or_else(|| site_key_from_url(site));
        match key {
            Some(key) => do_login(&key),
            None => eprintln!("Unknown site: {}", site),
        }
        return;
    }

    if args.len() != 4 {
        println!("Usage: submitter <url> <language> <file>");
        println!("       submitter login <url>");
        return;
    }

    let url = &args[1];
    let language = &args[2];
    let file = &args[3];
    let source = read_to_string(file).unwrap();

    let site_key = match site_key_from_url(url) {
        Some(key) => key,
        None => {
            println!("Unexpected URL");
            return;
        }
    };

    match site_key.as_str() {
        "codeforces.com" => codeforces::submit(url.clone(), source),
        "ucup.ac" => ucup::submit(url.clone(), language.clone(), source),
        "uoj.ac" | "qoj.ac" => {
            let (base, name, domain_str) = if site_key == "qoj.ac" {
                ("https://qoj.ac", "QOJ", "qoj.ac")
            } else {
                ("https://uoj.ac", "UOJ", "uoj.ac")
            };
            let mut client = submitter::uoj::UojClient::new(base, name);
            println!("Logging in");
            if let Err(e) = client.login() {
                eprintln!("Login failed: {}", e);
                return;
            }
            let path = url.find(domain_str).map(|p| &url[p + domain_str.len()..]).unwrap_or("/");
            println!("Submitting");
            if let Err(e) = client.submit(path, &language, &source) {
                eprintln!("Submit failed: {}", e);
            }
        }
        "yandex.com" | "yandex.ru" => yandex::submit(url.clone(), language.clone(), source),
        "codechef.com" => codechef::submit(url.clone(), language.clone(), source),
        "toph.co" => toph::submit(url.clone(), language.clone(), source),
        "kattis.com" => kattis::submit(url.clone(), language.clone(), source, file.clone()),
        "eolymp.com" => eolymp::submit(url.clone(), language.clone(), source),
        _ => println!("Unsupported domain: {}", site_key),
    }
}
