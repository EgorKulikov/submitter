mod atcoder;
mod codechef;
mod codeforces;
mod luogu;
mod toph;
mod ucup;
mod yandex;

use regex::Regex;
use std::collections::HashMap;
use std::env;
use std::fs::read_to_string;
use std::path::Path;
use std::process::Command;
use std::time::Duration;
use thirtyfour::prelude::*;
use which::which;

#[tokio::main]
async fn main() -> WebDriverResult<()> {
    let args: Vec<_> = env::args().collect();
    if args.len() != 4 {
        println!("Usage: submitter <url> <language> <file>");
        return Ok(());
    }
    let url = &args[1];
    let language = &args[2];
    let file = &args[3];
    let source = read_to_string(file).unwrap();

    let caps = DesiredCapabilities::chrome();

    let driver = match WebDriver::new("http://localhost:4444", caps.clone()).await {
        Ok(driver) => driver,
        Err(_) => {
            if which("docker").is_err() {
                println!("Please install docker");
                return Ok(());
            }
            println!("Selenium is not running, starting");
            let mut command = Command::new("docker");
            command.args(&[
                "run",
                "--rm",
                "-d",
                "-p",
                "4444:4444",
                "--name",
                "selenium-server",
                "-v",
                "//dev/shm:/dev/shm",
                "selenium/standalone-chrome:latest",
            ]);
            command.status().unwrap();
            println!("Waiting for selenium to start");
            tokio::time::sleep(Duration::from_secs(5)).await;
            WebDriver::new("http://localhost:4444", caps).await?
        }
    };

    run(&driver, &url, &language, &source).await?;

    driver.quit().await?;
    Ok(())
}

async fn run(
    driver: &WebDriver,
    url: &String,
    language: &String,
    source: &String,
) -> WebDriverResult<()> {
    let cookies_string = read_to_string("cookies.json").unwrap_or("{}".to_string());
    let mut all_cookies: HashMap<String, Vec<Cookie>> =
        serde_json::from_str(&cookies_string).unwrap_or(HashMap::new());
    let url_regex = Regex::new(r"https?://(?:www\.)?([^/]+).*").unwrap();
    let domain = {
        match url_regex.captures(url) {
            None => {
                println!("Unexpected URL");
                return Ok(());
            }
            Some(caps) => caps[1].to_string(),
        }
    };

    let site = match domain.as_str() {
        "codeforces.com" => Site::Codeforces,
        "codechef.com" => Site::Codechef,
        "contest.yandex.com" => Site::Yandex,
        "atcoder.jp" => Site::AtCoder,
        "contest.ucup.ac" => Site::UniversalCup,
        "luogu.com.cn" => {
            eprintln!("Luogu support is discontinued due to captcha");
            return Ok(());
        }
        "toph.co" => Site::Toph,
        _ => {
            println!("Unsupported domain");
            return Ok(());
        }
    };

    println!("Logging in");
    match site
        .login(&driver, all_cookies.get(&domain).cloned().unwrap_or(vec![]))
        .await
    {
        Ok(cookies) => {
            all_cookies.insert(domain, cookies.clone());
            let cookies_string = serde_json::to_string(&all_cookies).unwrap();
            std::fs::write("cookies.json", cookies_string).unwrap();
        }
        Err(err) => {
            all_cookies.insert(domain, Vec::new());
            let cookies_string = serde_json::to_string(&all_cookies).unwrap();
            std::fs::write("cookies.json", cookies_string).unwrap();
            eprintln!(
                "Failed to login:\n{}\n{:?}",
                driver.current_url().await?,
                err
            );
            return Ok(());
        }
    };
    println!("Submitting");
    site.submit(&driver, url.clone(), language.clone(), source.clone())
        .await?;
    Ok(())
}

enum Site {
    Codeforces,
    Codechef,
    Yandex,
    AtCoder,
    UniversalCup,
    // Luogu,
    Toph,
}

impl Site {
    async fn submit(
        &self,
        driver: &WebDriver,
        url: String,
        language: String,
        source: String,
    ) -> WebDriverResult<()> {
        match self {
            Site::Codeforces => codeforces::submit(driver, url, language, source).await,
            Site::Codechef => codechef::submit(driver, url, language, source).await,
            Site::Yandex => yandex::submit(driver, url, language, source).await,
            Site::AtCoder => atcoder::submit(driver, url, language, source).await,
            Site::UniversalCup => ucup::submit(driver, url, language, source).await,
            // Site::Luogu => luogu::submit(driver, url, language, source).await,
            Site::Toph => toph::submit(driver, url, language, source).await,
        }
    }

    async fn login(
        &self,
        driver: &WebDriver,
        cookies: Vec<Cookie>,
    ) -> WebDriverResult<Vec<Cookie>> {
        match self {
            Site::Codeforces => codeforces::login(driver, cookies).await,
            Site::Codechef => codechef::login(driver, cookies).await,
            Site::Yandex => yandex::login(driver, cookies).await,
            Site::AtCoder => atcoder::login(driver, cookies).await,
            Site::UniversalCup => ucup::login(driver, cookies).await,
            // Site::Luogu => luogu::login(driver, cookies).await,
            Site::Toph => toph::login(driver, cookies).await,
        }
    }
}

async fn select_value(selector: WebElement, value: &str) -> WebDriverResult<bool> {
    selector.focus().await?;
    let mut last = selector.value().await?;
    loop {
        if last == Some(value.to_string()) {
            return Ok(true);
        }
        selector.send_keys(Key::Down).await?;
        if last == selector.value().await? {
            break;
        }
        last = selector.value().await?;
    }
    loop {
        if last == Some(value.to_string()) {
            return Ok(true);
        }
        selector.send_keys(Key::Up).await?;
        if last == selector.value().await? {
            break;
        }
        last = selector.value().await?;
    }
    Ok(false)
}

async fn set_value(driver: &WebDriver, element: WebElement, value: String) -> WebDriverResult<()> {
    driver
        .execute(
            "arguments[0].value = arguments[1];",
            vec![element.to_json()?, serde_json::to_value(value).unwrap()],
        )
        .await?;
    Ok(())
}

#[allow(dead_code)]
async fn save_source(driver: &WebDriver) -> WebDriverResult<()> {
    driver.screenshot(&Path::new("screenshot.png")).await?;
    std::fs::write("source.html", driver.source().await?).unwrap();
    Ok(())
}

fn clear(len: usize) {
    for _ in 0..len {
        print!("{}", 8u8 as char);
    }
    for _ in 0..len {
        print!(" ");
    }
    for _ in 0..len {
        print!("{}", 8u8 as char);
    }
}
