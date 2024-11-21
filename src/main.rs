mod codeforces;
mod codechef;
mod yandex;

use regex::Regex;
use std::collections::HashMap;
use std::env;
use std::fs::read_to_string;
use std::process::Command;
use std::time::Duration;
use thirtyfour::prelude::*;
use which::which;

#[tokio::main]
async fn main() -> WebDriverResult<()> {
    let args: Vec<_> = env::args().collect();
    if args.len() != 4 {
        println!("Usage: cfsubmitter <url> <language> <file>");
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
            command.args(&["run", "--rm", "-d", "-p", "4444:4444", "-p", "5900:5900", "--name", "selenium-server", "-v", "//dev/shm:/dev/shm", "selenium/standalone-chrome:latest"]);
            command.status().unwrap();
            println!("Waiting for selenium to start");
            tokio::time::sleep(Duration::from_secs(5)).await;
            WebDriver::new("http://localhost:4444", caps).await?
        }
    };

    driver.set_page_load_timeout(Duration::from_secs(10)).await?;

    run(&driver, &url, &language, &source).await?;

    driver.quit().await?;
    Ok(())
}

async fn run(driver: &WebDriver, url: &String, language: &String, source: &String) -> WebDriverResult<()> {
    let cookies_string = std::fs::read_to_string("cookies.json").unwrap_or("{}".to_string());
    let mut all_cookies: HashMap<String, Vec<Cookie>> = serde_json::from_str(&cookies_string).unwrap_or(HashMap::new());
    let url_regex = Regex::new(r"https?://(?:www\.)?([^/]+).*").unwrap();
    let domain = {
        match url_regex.captures(url) {
            None => {
                println!("Unexpected URL");
                return Ok(());
            }
            Some(caps) =>
                caps[1].to_string(),
        }
    };


    let site = match domain.as_str() {
        "codeforces.com" => Site::Codeforces,
        "codechef.com" => Site::Codechef,
        "contest.yandex.com" => Site::Yandex,
        _ => {
            println!("Unsupported domain");
            return Ok(());
        }
    };

    println!("Logging in");
    let cookies = site.login(&driver, all_cookies.get(&domain).cloned().unwrap_or(vec![])).await?;
    all_cookies.insert(domain, cookies.clone());
    println!("Logged in, saving cookies");
    let cookies_string = serde_json::to_string(&all_cookies).unwrap();
    std::fs::write("cookies.json", cookies_string).unwrap();
    println!("Submitting");
    site.submit(&driver, url.clone(), language.clone(), source.clone()).await?;
    Ok(())
}

enum Site {
    Codeforces,
    Codechef,
    Yandex,
}

impl Site {
    async fn submit(&self, driver: &WebDriver, url: String, language: String, source: String) -> WebDriverResult<()> {
        match self {
            Site::Codeforces => codeforces::submit(driver, url, language, source).await,
            Site::Codechef => codechef::submit(driver, url, language, source).await,
            Site::Yandex => yandex::submit(driver, url, language, source).await,
        }
    }

    async fn login(&self, driver: &WebDriver, cookies: Vec<Cookie>) -> WebDriverResult<Vec<Cookie>> {
        match self {
            Site::Codeforces => codeforces::login(driver, cookies).await,
            Site::Codechef => codechef::login(driver, cookies).await,
            Site::Yandex => yandex::login(driver, cookies).await,
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
    driver.execute("arguments[0].value = arguments[1];", vec![element.to_json()?, serde_json::to_value(value).unwrap()]).await?;
    Ok(())
}
