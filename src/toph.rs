use crate::clear;
use crossterm::execute;
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use dialoguer::console::Term;
use dialoguer::{Input, Password};
use thirtyfour::error::{WebDriverError, WebDriverErrorInner, WebDriverResult};
use thirtyfour::{By, Cookie, WebDriver};

pub async fn login(driver: &WebDriver, cookies: Vec<Cookie>) -> WebDriverResult<Vec<Cookie>> {
    driver.goto("https://toph.co").await?;
    driver.delete_all_cookies().await?;
    for cookie in cookies {
        driver.add_cookie(cookie).await?;
    }
    driver.goto("https://toph.co/login").await?;
    if driver.current_url().await?.as_str() != "https://toph.co/login" {
        return Ok(driver.get_all_cookies().await?);
    }
    let login: String = Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Enter your toph login")
        .interact_on(&Term::stdout())
        .unwrap();
    let password: String = Password::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Enter your toph password")
        .interact_on(&Term::stdout())
        .unwrap();
    let inputs = driver.find_all(By::Tag("input")).await?;
    if inputs.len() != 2 {
        println!("Failed to find login and password inputs");
        return Err(WebDriverError::ParseError(
            "Failed to find login and password inputs".to_string(),
        ));
    }
    inputs[0].send_keys(login).await?;
    inputs[1].send_keys(password).await?;
    driver.find(By::Tag("button")).await?.click().await?;
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    Ok(driver.get_all_cookies().await?)
}

pub async fn submit(
    driver: &WebDriver,
    url: String,
    _language: String,
    source: String,
) -> WebDriverResult<()> {
    println!("Cannot change language on toph, language of last submit would be used");
    driver.maximize_window().await?;
    driver.goto(&url).await?;
    for button in driver.find_all(By::Tag("button")).await? {
        let class_name = button.class_name().await?;
        if class_name.is_some() && class_name.unwrap().contains("btn-codepanel") {
            button.click().await?;
            break;
        }
    }
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let source = escape_html(&source);
    driver
        .execute(
            "document.getElementsByClassName('cm-content')[0].innerHTML = arguments[0];",
            vec![serde_json::to_value(source).unwrap()],
        )
        .await?;
    let codepanel = driver.find(By::ClassName("codepanel")).await?;
    let buttons = codepanel.find_all(By::Tag("button")).await?;
    if buttons.len() < 14 {
        println!("Failed to find submit button");
        return Ok(());
    }
    buttons[13].click().await?;
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    if driver.current_url().await?.as_str().contains("/p/") {
        let toast = driver.find(By::ClassName("toast")).await?;
        println!("Error submitting: {}", toast.text().await?);
        return Ok(());
    }
    println!("Submission url {}", driver.current_url().await?);
    let mut last_verdict = "".to_string();
    loop {
        match single_iteration(driver, &mut last_verdict).await {
            Ok(true) => break,
            Ok(false) => continue,
            Err(err) => match *err {
                WebDriverErrorInner::NoSuchElement(_) => {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    continue;
                }
                WebDriverErrorInner::StaleElementReference(_) => {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    continue;
                }
                _ => {
                    return Err(err);
                }
            },
        }
    }
    Ok(())
}

async fn single_iteration(driver: &WebDriver, last_verdict: &mut String) -> WebDriverResult<bool> {
    let table = driver.find(By::ClassName("table")).await?;
    let rows = table.find_all(By::Tag("tr")).await?;
    if rows.len() < 2 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        return Ok(false);
    }
    let row = &rows[1];
    let columns = row.find_all(By::Tag("td")).await?;
    if columns.len() < 6 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        return Ok(false);
    }
    let verdict = columns[5].find(By::Tag("span")).await?;
    let mut verdict_text = verdict
        .inner_html()
        .await?
        .replace("<span class=\"font-muted\">", "")
        .replace("</span>", "")
        .replace("\n", "")
        .replace("\r", "")
        .replace("\t", " ")
        .trim()
        .to_string();
    while verdict_text.contains("  ") {
        verdict_text = verdict_text.replace("  ", " ");
    }
    let class_name = verdict.class_name().await?.unwrap_or("".to_string());
    // println!("{:?}", class_name);
    let (is_accepted, is_done) = if class_name.contains("font-red") {
        (false, true)
    } else if class_name.contains("font-green") {
        (true, true)
    } else {
        (false, false)
    };
    let verdict = verdict_text;
    clear(last_verdict.len());
    let mut stdout = std::io::stdout();
    let _ = execute!(
        stdout,
        SetForegroundColor(if !is_done {
            Color::Yellow
        } else if is_accepted {
            Color::Green
        } else {
            Color::Red
        })
    );
    print!("{}", verdict);
    let _ = execute!(stdout, ResetColor);
    if is_done {
        println!();
        return Ok(true);
    }
    *last_verdict = verdict;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    Ok(false)
}

fn escape_html(source: &String) -> String {
    source
        .replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace("\"", "&quot;")
        .replace("'", "&#39;")
}
