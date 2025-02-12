use crate::clear;
use crossterm::execute;
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use dialoguer::console::Term;
use dialoguer::{Input, Password};
use thirtyfour::error::{WebDriverError, WebDriverErrorInner, WebDriverResult};
use thirtyfour::{By, Cookie, WebDriver};

async fn is_cloudflare(driver: &WebDriver) -> WebDriverResult<bool> {
    Ok(driver.source().await?.contains(
        "<body><p>Please wait. Your browser is being checked. It may take a few seconds...</p>",
    ))
}

async fn skip_cloudflare(driver: &WebDriver) -> WebDriverResult<()> {
    let mut times = 0;
    while is_cloudflare(driver).await? {
        times += 1;
        if times == 10 {
            eprintln!("Cannot bypass cloudflare captcha, please submit manually");
            eprintln!("Will clear cookies, may help");
            return Err(WebDriverError::ParseError(
                "Cannot bypass cloudflare captcha".to_string(),
            ));
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    Ok(())
}

pub async fn login(driver: &WebDriver, cookies: Vec<Cookie>) -> WebDriverResult<Vec<Cookie>> {
    driver.goto("https://mirror.codeforces.com/").await?;
    driver.delete_all_cookies().await?;
    for cookie in cookies {
        driver.add_cookie(cookie).await?;
    }
    driver.goto("https://mirror.codeforces.com/enter").await?;
    skip_cloudflare(driver).await?;
    if driver.current_url().await?.as_str() != "https://mirror.codeforces.com/enter" {
        return Ok(driver.get_all_cookies().await?);
    }
    let login: String = Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Enter your codeforces login")
        .interact_on(&Term::stdout())
        .unwrap();
    let password: String = Password::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Enter your codeforces password")
        .interact_on(&Term::stdout())
        .unwrap();
    driver
        .find(By::Id("handleOrEmail"))
        .await?
        .send_keys(login)
        .await?;
    driver
        .find(By::Id("password"))
        .await?
        .send_keys(password)
        .await?;
    driver.find(By::Id("remember")).await?.click().await?;
    driver.find(By::ClassName("submit")).await?.click().await?;
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    skip_cloudflare(driver).await?;
    Ok(driver.get_all_cookies().await?)
}

pub async fn submit(
    driver: &WebDriver,
    url: String,
    language: String,
    source: String,
) -> WebDriverResult<()> {
    let pos = match url.rfind("/problem/") {
        None => {
            eprintln!("Bad url");
            return Ok(());
        }
        Some(pos) => pos,
    };
    let id = url[pos + 9..].replace("/", "");
    let (submit_url, status_url) = if url.contains("problemset") {
        let slash = url[pos + 9..].find('/').unwrap();
        (
            "https://mirror.codeforces.com/problemset/submit".to_string(),
            format!(
                "https://codeforces.com/problemset/submission/{}/",
                &url[pos + 9..pos + 9 + slash]
            ),
        )
    } else {
        (
            url[..pos].replace("https://codeforces.com", "https://mirror.codeforces.com")
                + "/submit",
            format!("{}/submission/", &url[..pos]),
        )
    };
    driver.goto(&submit_url).await?;
    skip_cloudflare(driver).await?;
    match driver.find(By::Name("submittedProblemCode")).await {
        Ok(element) => {
            element.send_keys(id).await?;
        }
        Err(_) => {
            let selector = driver.find(By::Name("submittedProblemIndex")).await?;
            if !crate::select_value(selector, id.as_str()).await? {
                eprintln!("Bad id");
                return Ok(());
            }
        }
    }
    let element = driver.find(By::Name("programTypeId")).await?;
    if !crate::select_value(element, get_language(language).as_str()).await? {
        eprintln!("Bad language");
        return Ok(());
    }
    driver
        .find(By::Id("toggleEditorCheckbox"))
        .await?
        .click()
        .await?;
    let input_field = driver.find(By::Id("sourceCodeTextarea")).await?;
    crate::set_value(driver, input_field, source).await?;
    driver.find(By::ClassName("submit")).await?.click().await?;
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    skip_cloudflare(driver).await?;
    if driver
        .current_url()
        .await?
        .as_str()
        .starts_with(&submit_url)
    {
        let error = driver.find_all(By::ClassName("error")).await?;
        eprintln!("Error submitting: ");
        for element in error {
            eprint!("{}", element.text().await?);
        }
        return Ok(());
    }
    let mut last_verdict = "".to_string();
    let mut printed_url = false;
    loop {
        clear(last_verdict.len());
        if !printed_url {
            if let Ok(id_cell) = driver.find(By::ClassName("id-cell")).await {
                if let Some(id) = id_cell
                    .find(By::Tag("a"))
                    .await?
                    .attr("submissionid")
                    .await?
                {
                    printed_url = true;
                    println!("Submission url {}{}", status_url, id);
                }
            }
        }
        match iteration(driver, &mut last_verdict).await {
            Ok(res) => {
                if res {
                    break;
                }
            }
            Err(err) => match *err {
                WebDriverErrorInner::NoSuchElement(_) => {}
                WebDriverErrorInner::StaleElementReference(_) => {}
                _ => {
                    return Err(err);
                }
            },
        }
    }
    Ok(())
}

async fn iteration(driver: &WebDriver, last_verdict: &mut String) -> WebDriverResult<bool> {
    let mut stdout = std::io::stdout();
    let cell = driver.find(By::ClassName("status-cell")).await?;
    let verdict = cell.text().await?;
    let (is_waiting, is_accepted) = match cell.find(By::Tag("span")).await {
        Ok(mut verdict) => {
            if verdict.class_name().await? == Some("submissionVerdictWrapper".to_string()) {
                verdict = verdict.find(By::Tag("span")).await?;
            }
            (
                verdict.class_name().await? == Some("verdict-waiting".to_string()),
                verdict.class_name().await? == Some("verdict-accepted".to_string()),
            )
        }
        Err(_) => {
            if verdict.trim() == "Compilation error" {
                (false, false)
            } else {
                (true, false)
            }
        }
    };
    let _ = execute!(
        stdout,
        SetForegroundColor(if is_waiting {
            Color::Yellow
        } else if is_accepted {
            Color::Green
        } else {
            Color::Red
        })
    );
    print!("{}", verdict);
    let _ = execute!(stdout, ResetColor);
    if verdict == *last_verdict && is_waiting {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        driver.refresh().await?;
        skip_cloudflare(driver).await?;
        return Ok(false);
    }
    if !is_waiting {
        println!();
        return Ok(true);
    }
    *last_verdict = verdict;
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    Ok(false)
}

fn get_language(language: String) -> String {
    match language.to_lowercase().as_str() {
        "c++" | "c++20" => "89".to_string(),
        "c++17" => "54".to_string(),
        "c++23" => "91".to_string(),
        "c" => "43".to_string(),
        "c#" | "c#10" => "79".to_string(),
        "c#8" => "79".to_string(),
        "c#mono" => "9".to_string(),
        "d" => "28".to_string(),
        "go" => "32".to_string(),
        "haskell" => "12".to_string(),
        "java" | "java21" => "87".to_string(),
        "java8" => "83".to_string(),
        "kotlin" | "kotlin1.9" => "88".to_string(),
        "kotlin1.7" => "83".to_string(),
        "ocaml" => "19".to_string(),
        "delphi" => "3".to_string(),
        "pascal" | "freepascal" => "4".to_string(),
        "pascalabc" => "51".to_string(),
        "perl" => "13".to_string(),
        "php" => "6".to_string(),
        "python" | "python3" => "31".to_string(),
        "python2" => "7".to_string(),
        "pypy" | "pypy3" => "70".to_string(),
        "pypy3x32" => "41".to_string(),
        "pypy2" => "40".to_string(),
        "ruby" => "67".to_string(),
        "rust" => "75".to_string(),
        "scala" => "20".to_string(),
        "javascript" | "js" => "34".to_string(),
        "node.js" | "node" => "55".to_string(),
        _ => language,
    }
}
