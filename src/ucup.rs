use crate::{clear, set_value};
use crossterm::execute;
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use dialoguer::console::Term;
use dialoguer::{Input, Password};
use thirtyfour::error::{WebDriverErrorInner, WebDriverResult};
use thirtyfour::{By, Cookie, WebDriver};

pub async fn login(driver: &WebDriver, cookies: Vec<Cookie>) -> WebDriverResult<Vec<Cookie>> {
    driver.goto("https://contest.ucup.ac/").await?;
    driver.delete_all_cookies().await?;
    for cookie in cookies {
        driver.add_cookie(cookie).await?;
    }
    driver.goto("https://contest.ucup.ac/login").await?;
    if driver.current_url().await?.as_str() != "https://contest.ucup.ac/login" {
        return Ok(driver.get_all_cookies().await?);
    }
    let login: String = Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Enter your universal cup login")
        .interact_on(&Term::stdout())
        .unwrap();
    let password: String = Password::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Enter your universal cup password")
        .interact_on(&Term::stdout())
        .unwrap();
    driver
        .find(By::Id("input-username"))
        .await?
        .send_keys(login)
        .await?;
    driver
        .find(By::Id("input-password"))
        .await?
        .send_keys(password)
        .await?;
    driver.find(By::Id("button-submit")).await?.click().await?;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    Ok(driver.get_all_cookies().await?)
}

pub async fn submit(
    driver: &WebDriver,
    url: String,
    language: String,
    source: String,
) -> WebDriverResult<()> {
    driver.goto(&url).await?;
    driver
        .find(By::PartialLinkText("Submit"))
        .await?
        .click()
        .await?;
    let language_selector = driver.find(By::Id("input-answer_answer_language")).await?;
    let options = language_selector.find_all(By::Tag("option")).await?;
    let mut result = "".to_string();
    for option in options {
        let value = option.value().await?;
        if let Some(option) = value {
            if option.to_lowercase().starts_with(&language.to_lowercase()) {
                result = option;
            }
        }
    }
    if result.is_empty() {
        println!("Language not found");
        return Ok(());
    }
    set_value(driver, language_selector, result).await?;
    let source_code = driver.find(By::Id("input-answer_answer_editor")).await?;
    set_value(driver, source_code, source).await?;
    driver
        .find(By::Id("button-submit-answer"))
        .await?
        .click()
        .await?;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let mut last_verdict = "".to_string();
    loop {
        match iteration(driver, &mut last_verdict).await {
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

async fn iteration(driver: &WebDriver, last_submit: &mut String) -> WebDriverResult<bool> {
    let mut stdout = std::io::stdout();
    match driver.find(By::ClassName("info")).await {
        Ok(info) => {
            let verdict = info
                .find(By::ClassName("uoj-status-details-text-div"))
                .await?
                .text()
                .await?;
            clear(last_submit.len());
            let _ = execute!(stdout, SetForegroundColor(Color::Yellow));
            print!("{}", verdict);
            let _ = execute!(stdout, ResetColor);
            *last_submit = verdict;
            Ok(false)
        }
        Err(_) => {
            let verdict = driver
                .find(By::ClassName("uoj-score"))
                .await?
                .text()
                .await?;
            clear(last_submit.len());
            let _ = execute!(
                stdout,
                SetForegroundColor(if verdict.starts_with("AC") {
                    Color::Green
                } else {
                    Color::Red
                })
            );
            println!("{}", verdict);
            let _ = execute!(stdout, ResetColor);
            let rows = driver.find_all(By::Tag("tr")).await?;
            if rows.len() >= 2 {
                let link = rows[1].find(By::Tag("a")).await?;
                if let Some(link) = link.attr("href").await? {
                    println!("Submission url https://contest.ucup.ac{}", link);
                }
            }
            Ok(true)
        }
    }
}
