use crate::clear;
use crossterm::execute;
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use dialoguer::console::Term;
use dialoguer::{Input, Password};
use thirtyfour::error::WebDriverResult;
use thirtyfour::{By, Cookie, Key, WebDriver};

pub async fn login(driver: &WebDriver, cookies: Vec<Cookie>) -> WebDriverResult<Vec<Cookie>> {
    driver.goto("https://codechef.com/").await?;
    driver.delete_all_cookies().await?;
    for cookie in cookies {
        driver.add_cookie(cookie).await?;
    }
    driver.goto("https://codechef.com/").await?;
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    let source = driver.source().await?;
    if !source.contains("Sign Up") {
        return Ok(driver.get_all_cookies().await?);
    }
    driver.goto("https://www.codechef.com/login").await?;
    let login: String = Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Enter your codechef login")
        .interact_on(&Term::stdout())
        .unwrap();
    let password: String = Password::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Enter your codechef password")
        .interact_on(&Term::stdout())
        .unwrap();
    driver
        .action_chain()
        .send_keys(login)
        .send_keys(Key::Tab)
        .perform()
        .await?;
    driver
        .action_chain()
        .send_keys(password)
        .send_keys(Key::Tab)
        .perform()
        .await?;
    driver.action_chain().send_keys(" ").perform().await?;
    for _ in 0..10 {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        if driver.current_url().await?.as_str() != "https://www.codechef.com/login" {
            eprintln!("Logged in");
            return Ok(driver.get_all_cookies().await?);
        }
    }
    eprintln!("Failed to login");
    Err(thirtyfour::error::WebDriverError::ParseError(
        "Failed to login".to_string(),
    ))
}

pub async fn submit(
    driver: &WebDriver,
    url: String,
    language: String,
    source: String,
) -> WebDriverResult<()> {
    driver.maximize_window().await?;
    driver.goto(&url).await?;
    tokio::time::sleep(std::time::Duration::from_secs(4)).await;
    let language_select = driver.find(By::Id("language-select")).await?;
    language_select.click().await?;
    driver.action_chain().send_keys(language).perform().await?;
    let center = language_select.rect().await?.icenter();
    driver
        .action_chain()
        .move_to(center.0, center.1 + 80)
        .click()
        .perform()
        .await?;
    driver
        .execute(
            "\
        var editordiv = document.getElementById(\"submit-ide-v2\");\
        var editor = ace.edit(editordiv);\
        editor.setValue(arguments[0]);\
    ",
            vec![serde_json::to_value(source).unwrap()],
        )
        .await?;
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    driver.find(By::Id("submit_btn")).await?.click().await?;
    let mut stdout = std::io::stdout();
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    driver
        .find(By::Id("vertical-tab-panel-1"))
        .await?
        .click()
        .await?;
    tokio::time::sleep(std::time::Duration::from_secs(4)).await;
    let id = driver
        .find_all(By::Tag("tbody"))
        .await?
        .last()
        .unwrap()
        .find_all(By::Tag("div"))
        .await?[1]
        .text()
        .await?;
    driver
        .goto(&format!("https://www.codechef.com/viewsolution/{}", id))
        .await?;
    println!(
        "Submission url https://www.codechef.com/viewsolution/{}",
        id
    );
    let _ = execute!(stdout, SetForegroundColor(Color::Yellow));
    print!("Judging");
    let _ = execute!(stdout, ResetColor);
    tokio::time::sleep(std::time::Duration::from_secs(4)).await;
    loop {
        let verdict = driver
            .find(By::ClassName("_status__container_1xnpw_48"))
            .await?;
        if verdict.text().await?.as_str() == "Submission Queued..." {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            continue;
        }
        clear(7);
        let full_verdict = verdict.find(By::Tag("span")).await?.text().await?;
        let accepted = full_verdict.contains("Correct Answer")
            || full_verdict.contains("You got it right!")
            || full_verdict.contains("Excellent work!")
            || full_verdict.contains("Awesome, you nailed it!");
        let _ = execute!(
            stdout,
            SetForegroundColor(if accepted { Color::Green } else { Color::Red })
        );
        println!("{}", full_verdict);
        let _ = execute!(stdout, ResetColor);
        if full_verdict == "Compilation Error".to_string() {
            return Ok(());
        }
        let table = loop {
            match driver.find(By::ClassName("status-table")).await {
                Ok(table) => break table,
                Err(_) => {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }
        };
        let rows = table.find_all(By::Tag("tr")).await?;
        println!("Subtask Task Result");
        for row in rows.into_iter().skip(1) {
            if row.class_name().await? == Some("skip".to_string())
                || row.class_name().await? == Some("subtask-result".to_string())
            {
                continue;
            }
            let is_accepted = row.class_name().await? == Some("correct".to_string());
            let cells = row.find_all(By::Tag("td")).await?;
            if cells.len() < 3 {
                continue;
            }
            let subtask = cells[0].text().await?;
            let task = cells[1].text().await?;
            let result = cells[2]
                .text()
                .await?
                .replace("\n", "")
                .replace("\"", "")
                .replace("<br>", " ");
            let _ = execute!(
                stdout,
                SetForegroundColor(if is_accepted {
                    Color::Green
                } else {
                    Color::Red
                })
            );
            println!("{:7} {:4} {}", subtask, task, result);
            let _ = execute!(stdout, ResetColor);
        }
        break;
    }
    Ok(())
}
