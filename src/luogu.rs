#![allow(dead_code)]

use crate::{clear, save_source};
use crossterm::execute;
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use dialoguer::console::Term;
use dialoguer::{Input, Password};
use std::collections::BTreeSet;
use std::path::Path;
use thirtyfour::error::{WebDriverErrorInner, WebDriverResult};
use thirtyfour::{By, Cookie, WebDriver};

pub async fn login(driver: &WebDriver, cookies: Vec<Cookie>) -> WebDriverResult<Vec<Cookie>> {
    driver.goto("https://www.luogu.com.cn/auth/login").await?;
    driver.delete_all_cookies().await?;
    for cookie in cookies {
        driver.add_cookie(cookie).await?;
    }
    driver.goto("https://www.luogu.com.cn/auth/login").await?;
    if driver.current_url().await?.as_str() != "https://www.luogu.com.cn/auth/login" {
        return Ok(driver.get_all_cookies().await?);
    }
    let inputs = driver.find_all(By::Tag("input")).await?;
    let captchas = driver.find_all(By::Tag("img")).await?;
    for captcha in captchas {
        if let Some(src) = captcha.attr("src").await? {
            if src.contains("captcha") {
                captcha.screenshot(&Path::new("captcha.png")).await?;
            }
        }
    }
    let login: String = Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Enter your luogo login")
        .interact_on(&Term::stdout())
        .unwrap();
    let password: String = Password::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Enter your luogo password")
        .interact_on(&Term::stdout())
        .unwrap();
    let captcha: String = Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Enter the captcha from captcha.png")
        .interact_on(&Term::stdout())
        .unwrap();
    for input in inputs {
        if let Some(placeholder) = input.attr("placeholder").await? {
            match placeholder.as_str() {
                "用户名、手机号或电子邮箱" => {
                    input.send_keys(&login).await?;
                }
                "密码" => {
                    input.send_keys(&password).await?;
                }
                "右侧图形验证码" => {
                    input.send_keys(&captcha).await?;
                }
                _ => {}
            }
        }
    }
    driver
        .find(By::ClassName("btn-login"))
        .await?
        .click()
        .await?;
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    Ok(driver.get_all_cookies().await?)
}

pub async fn submit(
    driver: &WebDriver,
    url: String,
    _language: String,
    source: String,
) -> WebDriverResult<()> {
    println!("Cannot change language on luogo, language of last submit would be used");
    driver.goto(&url).await?;
    driver.find(By::ClassName("solid")).await?.click().await?;
    driver
        .execute(
            "document.getElementsByClassName('cm-content')[0].innerHTML = arguments[0];",
            vec![serde_json::to_value(source).unwrap()],
        )
        .await?;
    let buttons = driver.find_all(By::Tag("button")).await?;
    for button in buttons {
        if button.text().await? == "提交评测" {
            button.click().await?;
            break;
        }
    }
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    driver.screenshot(&Path::new("screenshot.png")).await?;
    save_source(driver).await?;
    eprintln!("Url: {}", driver.current_url().await?);
    let mut url_printed = false;
    let mut last_verdict = "".to_string();
    let mut tries = 0;
    loop {
        if !url_printed {
            let url = driver.current_url().await?.to_string();
            if url.starts_with("https://www.luogu.com.cn/record/") {
                clear(last_verdict.len());
                last_verdict = String::new();
                println!("Submission url {}", url);
                url_printed = true;
            }
        }
        match iteration(driver, &mut last_verdict, &mut tries).await {
            Ok(true) => break,
            Err(err) => match *err {
                WebDriverErrorInner::StaleElementReference(_) => {
                    continue;
                }
                _ => {
                    println!("Error while checking verdict");
                    break;
                }
            },
            _ => {}
        }
    }
    Ok(())
}

async fn iteration(
    driver: &WebDriver,
    last_verdict: &mut String,
    tries: &mut usize,
) -> WebDriverResult<bool> {
    if let Ok(content_el) = driver.find(By::Id("swal2-content")).await {
        let content = content_el.inner_html().await?.trim().to_string();
        if content.is_empty() {
            *tries += 1;
            if *tries == 50 {
                driver.refresh().await?;
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            return Ok(false);
        }
        if content.contains("captcha") {
            content_el.screenshot(&Path::new("captcha.png")).await?;
            let captcha: String = Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
                .with_prompt("Enter the captcha from captcha.png")
                .interact_on(&Term::stdout())
                .unwrap();
            let input = driver.find(By::ClassName("swal2-input")).await?;
            input.send_keys(&captcha).await?;
            let button = driver.find(By::ClassName("swal2-confirm")).await?;
            button.click().await?;
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            return Ok(false);
        }
        println!("Error from luogo, probably code is too long: {}", content);
        return Ok(true);
    }
    let (global_verdict, points) = if let Ok(side) = driver.find(By::ClassName("side")).await {
        let spans = side.find_all(By::Tag("span")).await?;
        let mut global_verdict = None;
        let mut points = None;
        for i in 0..spans.len() {
            if spans[i].text().await?.contains("评测状态") && i + 2 < spans.len() {
                global_verdict = Some(spans[i + 2].text().await?.trim().to_string());
            }
            if spans[i].text().await?.contains("评测分数") && i + 3 < spans.len() {
                points = Some(spans[i + 3].text().await?.trim().to_string());
            }
        }
        (global_verdict, points)
    } else {
        *tries += 1;
        if *tries == 50 {
            driver.refresh().await?;
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        return Ok(false);
    };
    clear(last_verdict.len());
    if global_verdict == Some("Compile Error".to_string()) {
        let _ = execute!(std::io::stdout(), SetForegroundColor(Color::Red));
        println!("Compile Error");
        let _ = execute!(std::io::stdout(), ResetColor);
        return Ok(true);
    }
    let mut subtasks = driver.find_all(By::ClassName("test-case-wrap")).await?;
    if subtasks.is_empty() {
        subtasks = driver.find_all(By::ClassName("main")).await?;
    }
    let mut cards = Vec::new();
    let mut verdicts = BTreeSet::new();
    let mut pending = 0;
    let mut total = 0;
    for subtask in subtasks {
        let name = match subtask.find(By::Tag("h5")).await {
            Ok(h) => h.text().await?.trim().to_string(),
            Err(_) => "All tests".to_string(),
        };
        let tests = subtask.find_all(By::ClassName("content")).await?;
        let mut cur = Vec::new();
        for test in tests {
            total += 1;
            if test.find(By::ClassName("spinner")).await.is_ok() {
                pending += 1;
                cur.push("Judging".to_string());
            } else {
                let verdict = test
                    .find(By::ClassName("status"))
                    .await?
                    .text()
                    .await?
                    .trim()
                    .to_string();
                if verdict != "AC" && !verdict.is_empty() {
                    verdicts.insert(verdict.clone());
                }
                if verdict.is_empty() {
                    pending += 1;
                    cur.push("Judging".to_string());
                } else {
                    cur.push(verdict);
                }
            }
        }
        cards.push((name, cur));
    }
    let (mut verdict, color) = if total == 0 {
        ("Waiting".to_string(), Color::Yellow)
    } else if !verdicts.is_empty() {
        let mut all = String::new();
        for verdict in &verdicts {
            if !all.is_empty() {
                all.push_str(", ");
            }
            all.push_str(verdict);
        }
        (all, Color::Red)
    } else if pending != 0 {
        ("Judging".to_string(), Color::Yellow)
    } else {
        ("Accepted".to_string(), Color::Green)
    };
    if pending != 0 {
        verdict += &format!(" {}/{}", total - pending, total);
    }
    if let Some(points) = points {
        verdict += &format!(" ({} pts)", points);
    }
    let mut stdout = std::io::stdout();
    let _ = execute!(stdout, SetForegroundColor(color));
    print!("{}", verdict);
    let _ = execute!(stdout, ResetColor);
    if total != 0 && pending == 0 {
        println!();
        let mut id = 1;
        for (name, tests) in cards {
            println!("{}", name);
            for test in tests {
                print!("  Test #{}: ", id);
                id += 1;
                let _ = execute!(
                    stdout,
                    SetForegroundColor(if &test == "AC" {
                        Color::Green
                    } else {
                        Color::Red
                    })
                );
                println!("{}", test);
                let _ = execute!(stdout, ResetColor);
            }
        }
        Ok(true)
    } else {
        *tries += 1;
        if *last_verdict != verdict {
            *tries = 0;
        }
        if *tries == 50 {
            driver.refresh().await?;
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        *last_verdict = verdict;
        Ok(false)
    }
}
