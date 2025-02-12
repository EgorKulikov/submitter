use crate::{clear, set_value};
use crossterm::execute;
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use dialoguer::console::Term;
use dialoguer::{Input, Password};
use thirtyfour::error::{WebDriverErrorInner, WebDriverResult};
use thirtyfour::{By, Cookie, Key, WebDriver};

pub async fn login(driver: &WebDriver, cookies: Vec<Cookie>) -> WebDriverResult<Vec<Cookie>> {
    driver
        .goto("https://contest.yandex.com/contest/3/problems/B/")
        .await?;
    for cookie in cookies {
        driver.add_cookie(cookie).await?;
    }
    driver
        .goto("https://contest.yandex.com/contest/3/problems/B/")
        .await?;
    if !driver.source().await?.contains("log in") {
        return Ok(driver.get_all_cookies().await?);
    }
    driver.goto("https://passport.yandex.com/auth?origin=contest&retpath=http://contest.yandex.com/contest/3/enter/?retPage=").await?;
    let login: String = Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Enter your yandex login")
        .interact_on(&Term::stdout())
        .unwrap();
    driver
        .find(By::Id("passp-field-login"))
        .await?
        .send_keys(login)
        .await?;
    driver.find(By::Id("passp:sign-in")).await?.click().await?;
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    let password: String = Password::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Enter your yandex password")
        .interact_on(&Term::stdout())
        .unwrap();
    driver
        .find(By::Id("passp-field-passwd"))
        .await?
        .send_keys(password)
        .await?;
    driver.find(By::Id("passp:sign-in")).await?.click().await?;
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    let confirmation: String = Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Enter your confirmation code from email")
        .interact_on(&Term::stdout())
        .unwrap();
    driver
        .find(By::Id("passp-field-confirmation-code"))
        .await?
        .send_keys(confirmation)
        .await?;
    driver.find(By::ClassName("Button2")).await?.click().await?;
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    Ok(driver.get_all_cookies().await?)
}

pub async fn submit(
    driver: &WebDriver,
    url: String,
    language: String,
    source: String,
) -> WebDriverResult<()> {
    driver.goto(&url).await?;
    let language_selector = driver.find(By::ClassName("select__control")).await?;
    let options = language_selector.find_all(By::Tag("option")).await?;
    let mut value = "".to_string();
    for option in options {
        if option
            .inner_html()
            .await?
            .to_lowercase()
            .starts_with(&language.to_lowercase())
        {
            value = option.attr("value").await?.unwrap_or("".to_string());
            break;
        }
    }
    if value.is_empty() {
        println!("Language not found");
        return Ok(());
    }
    set_value(driver, language_selector.clone(), value).await?;
    driver
        .action_chain()
        .send_keys(Key::PageDown)
        .perform()
        .await?;
    let radio_button = driver.find(By::ClassName("radio-button__control")).await?;
    radio_button.focus().await?;
    radio_button.send_keys(Key::Space).await?;
    driver
        .execute(
            "\
        var textArea = document.getElementsByClassName('input__control')[0];\
        var editor = CodeMirror.fromTextArea(textArea);\
        editor.getDoc().setValue(arguments[0]);\
    ",
            vec![serde_json::to_value(source).unwrap()],
        )
        .await?;
    driver
        .action_chain()
        .send_keys(Key::PageDown)
        .perform()
        .await?;
    driver
        .action_chain()
        .send_keys(Key::PageDown)
        .perform()
        .await?;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    driver
        .action_chain()
        .send_keys(Key::PageDown)
        .perform()
        .await?;
    driver
        .find(By::ClassName("problem__send"))
        .await?
        .find(By::Tag("button"))
        .await?
        .click()
        .await?;
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
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
    if columns.len() < 9 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        return Ok(false);
    }
    let verdict = columns[4].find(By::ClassName("table__data")).await?;
    let class_name = verdict.class_name().await?.unwrap_or("".to_string());
    // println!("{:?}", class_name);
    let (is_accepted, is_done) = if class_name.contains("table__data_mood_neg") {
        (false, true)
    } else if class_name.contains("table__data_mood_pos") {
        (true, true)
    } else {
        (false, false)
    };
    let mut verdict = verdict.find(By::Tag("a")).await?.text().await?;
    let test = columns[8].text().await?;
    if test.as_str() != "-" {
        verdict += &format!(" on test {}", test);
    }
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
        if columns.len() > 10 {
            let link = columns[10].find(By::Tag("a")).await?;
            if let Some(href) = link.attr("href").await? {
                println!("Submission url https://contest.yandex.com{}", href);
            }
        }
        return Ok(true);
    }
    *last_verdict = verdict;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    Ok(false)
}
