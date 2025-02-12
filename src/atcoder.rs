use crate::clear;
use crossterm::execute;
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use dialoguer::console::Term;
use dialoguer::{Input, Password};
use regex::Regex;
use thirtyfour::error::{WebDriverErrorInner, WebDriverResult};
use thirtyfour::{By, Cookie, Key, WebDriver};

pub async fn login(driver: &WebDriver, cookies: Vec<Cookie>) -> WebDriverResult<Vec<Cookie>> {
    driver.goto("https://atcoder.jp").await?;
    driver.delete_all_cookies().await?;
    for cookie in cookies {
        driver.add_cookie(cookie).await?;
    }
    driver.goto("https://atcoder.jp/login").await?;
    if !driver
        .source()
        .await?
        .contains("var userScreenName = \"\";")
    {
        return Ok(driver.get_all_cookies().await?);
    }
    let login: String = Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Enter your atcoder login")
        .interact_on(&Term::stdout())
        .unwrap();
    let password: String = Password::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Enter your atcoder password")
        .interact_on(&Term::stdout())
        .unwrap();
    driver
        .find(By::Id("username"))
        .await?
        .send_keys(login)
        .await?;
    driver
        .find(By::Id("password"))
        .await?
        .send_keys(password)
        .await?;
    driver.find(By::Id("submit")).await?.click().await?;
    Ok(driver.get_all_cookies().await?)
}

pub async fn submit(
    driver: &WebDriver,
    url: String,
    language: String,
    source: String,
) -> WebDriverResult<()> {
    let regex = Regex::new(r#"https://atcoder.jp/contests/(\w+)/tasks/(\w+)"#).unwrap();
    let (contest_id, task_id) = match regex.captures(&url) {
        None => {
            println!("Invalid url");
            return Ok(());
        }
        Some(caps) => (caps[1].to_string(), caps[2].to_string()),
    };
    driver
        .goto(&format!(
            "https://atcoder.jp/contests/{}/submit?taskScreenName={}",
            contest_id, task_id
        ))
        .await?;
    let (x, y) = driver
        .find(By::Name("data.LanguageId"))
        .await?
        .rect()
        .await?
        .icenter();
    driver
        .action_chain()
        .move_to(x + 10, y + 10)
        .click()
        .perform()
        .await?;
    driver.action_chain().send_keys(language).perform().await?;
    driver
        .action_chain()
        .send_keys(Key::Enter)
        .perform()
        .await?;
    driver
        .execute(
            "\
        var editordiv = document.getElementById(\"editor\");\
        var editor = ace.edit(editordiv);\
        editor.setValue(arguments[0]);\
    ",
            vec![serde_json::to_value(source).unwrap()],
        )
        .await?;
    driver.find(By::Id("submit")).await?.click().await?;
    let mut last_verdict = "".to_string();
    let mut printed_url = false;
    let mut times = 0;
    loop {
        clear(last_verdict.len());
        if !printed_url {
            if let Ok(cell) = driver.find(By::ClassName("submission-details-link")).await {
                if let Some(url) = cell.attr("href").await? {
                    println!("Submission url https://atcoder.jp{}", url);
                    printed_url = true;
                }
            }
        }
        match iteration(driver, &mut last_verdict, &mut times).await {
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
    times: &mut usize,
) -> WebDriverResult<bool> {
    let table = driver.find(By::Tag("tbody")).await?;
    let row = table.find(By::Tag("tr")).await?;
    let cols = row.find_all(By::Tag("td")).await?;
    if cols.len() < 7 {
        println!("Page format changed?");
        return Ok(true);
    }
    let span = cols[6].find(By::Tag("span")).await?;
    let mut verdict = span.attr("data-original-title").await?;
    if verdict.is_none() {
        verdict = span.attr("title").await?;
    }
    if verdict.is_none() {
        println!("Page format changed?");
        return Ok(true);
    }
    let mut verdict = verdict.unwrap();
    let text = span.text().await?;
    if text.contains("/") {
        verdict += " ";
        verdict += text.split(" ").next().unwrap();
    }
    let class = span.class_name().await?;
    if class.is_none() {
        println!("Page format changed?");
        return Ok(true);
    }
    let class = class.unwrap();
    let mut stdout = std::io::stdout();
    let _ = execute!(
        stdout,
        SetForegroundColor(if class.contains("label-success") {
            Color::Green
        } else if class.contains("label-default") {
            Color::Yellow
        } else {
            Color::Red
        })
    );
    print!("{}", verdict);
    let _ = execute!(stdout, ResetColor);
    if !class.contains("label-default") {
        println!();
        return Ok(true);
    }
    if *last_verdict == verdict {
        *times += 1;
        if *times > 50 {
            driver.refresh().await?;
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            *times = 0;
        }
    } else {
        *times = 0;
    }
    *last_verdict = verdict;
    Ok(false)
}
