use crate::clear;
use crossterm::execute;
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use dialoguer::console::Term;
use dialoguer::{Input, Password};
use rand::{thread_rng, Rng};
use thirtyfour::error::WebDriverResult;
use thirtyfour::{By, Cookie, WebDriver};

async fn is_cloudflare(driver: &WebDriver) -> WebDriverResult<bool> {
    let result = driver.title().await?.as_str() == "Just a moment..." || driver.source().await?.contains("<body><p>Please wait. Your browser is being checked. It may take a few seconds...</p>");
    if driver.source().await?.contains("Verify you are human by completing the action below.") {
        println!("Captcha, trying to pass. Consider submitting manually");
        let mut x = thread_rng().gen_range(20i64..120);
        let mut y = thread_rng().gen_range(200i64..350);
        while (x - 53).abs() > 5 || (y - 291).abs() > 5 {
            driver.action_chain().move_to(x, y).perform().await?;
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            loop {
                let nx = x + thread_rng().gen_range(-5..5);
                let ny = y + thread_rng().gen_range(-5..5);
                if (nx - 53).abs() + (ny - 291).abs() < (x - 53).abs() + (y - 291).abs() {
                    x = nx;
                    y = ny;
                    break;
                }
            }
        }
        driver.action_chain().move_to(x, y).click().perform().await?;
    }
    Ok(result)
}

pub async fn login(driver: &WebDriver, cookies: Vec<Cookie>) -> WebDriverResult<Vec<Cookie>> {
    driver.goto("https://codeforces.com/").await?;
    let mut times = 0;
    while is_cloudflare(driver).await? {
        times += 1;
        if times > 10 {
            eprintln!("Cannot bypass cloudflare captcha, please submit manually");
            return Ok(vec![]);
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    driver.delete_all_cookies().await?;
    for cookie in cookies {
        driver.add_cookie(cookie).await?;
    }
    driver.goto("https://codeforces.com/enter").await?;
    while is_cloudflare(driver).await? {
        times += 1;
        if times > 1 {
            // eprintln!("Cannot bypass cloudflare captcha, please submit manually");
            return Ok(driver.get_all_cookies().await?);
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    if driver.current_url().await?.as_str() != "https://codeforces.com/enter" {
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
    driver.find(By::Id("handleOrEmail")).await?.send_keys(login).await?;
    driver.find(By::Id("password")).await?.send_keys(password).await?;
    driver.find(By::Id("remember")).await?.click().await?;
    driver.find(By::ClassName("submit")).await?.click().await?;
    while is_cloudflare(driver).await? || driver.current_url().await?.as_str() == "https://codeforces.com/enter" {
        times += 1;
        if times > 10 {
            eprintln!("Cannot bypass cloudflare captcha, please submit manually");
            return Ok(vec![]);
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    Ok(driver.get_all_cookies().await?)
}

pub async fn submit(driver: &WebDriver, url: String, language: String, source: String) -> WebDriverResult<()> {
    let pos = match url.rfind("/problem/") {
        None => {
            eprintln!("Bad url");
            return Ok(());
        }
        Some(pos) => pos,
    };
    let id = url[pos + 9..].replace("/", "");
    let submit_url = if url.contains("problemset") {
        "https://codeforces.com/problemset/submit".to_string()
    } else {
        url[..pos].to_string() + "/submit"
    };
    driver.goto(&submit_url).await?;
    let mut times = 0;
    while is_cloudflare(driver).await? {
        times += 1;
        if times > 10 {
            eprintln!("Cannot bypass cloudflare captcha, please submit manually");
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
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
    driver.find(By::Id("toggleEditorCheckbox")).await?.click().await?;
    let input_field = driver.find(By::Id("sourceCodeTextarea")).await?;
    crate::set_value(driver, input_field, source).await?;
    driver.find(By::ClassName("submit")).await?.click().await?;
    while is_cloudflare(driver).await? || driver.current_url().await?.as_str() == submit_url.as_str() {
        times += 1;
        if times > 10 {
            eprintln!("Cannot bypass cloudflare captcha, please submit manually");
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    if driver.current_url().await?.as_str().starts_with(&submit_url) {
        eprintln!("You already submitted this code");
        return Ok(());
    }
    let mut last_verdict = "".to_string();
    loop {
        let source = driver.source().await?;
        let pos = match source.find("\"status-cell status-small status-verdict-cell") {
            None => {
                eprintln!("Cannot fetch verdict");
                return Ok(());
            }
            Some(pos) => pos,
        };
        let rem = &source[pos..];
        let pos = match rem.find("waiting=\"") {
            None => {
                eprintln!("Cannot fetch verdict");
                return Ok(());
            }
            Some(pos) => pos + 9,
        };
        let rem = &rem[pos..];
        let is_waiting = rem.starts_with("true");
        let pos = match rem.find(">") {
            None => {
                eprintln!("Cannot fetch verdict");
                return Ok(());
            }
            Some(pos) => pos + 1,
        };
        let rem = &rem[pos..];
        let pos = match rem[pos..].find("</td>") {
            None => {
                eprintln!("Cannot fetch verdict");
                return Ok(());
            }
            Some(pos) => pos,
        };
        let verdict = rem[..=pos].to_string();
        let (verdict, is_accepted) = extract_verdict(&verdict);
        clear(last_verdict.len());
        if verdict == last_verdict && is_waiting {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            driver.refresh().await?;
            continue;
        }
        let mut stdout = std::io::stdout();
        let _ = execute!(stdout, SetForegroundColor(
            if is_waiting {
                Color::Yellow
            } else if is_accepted {
                Color::Green
            } else {
                Color::Red
            }
        ));
        print!("{}", verdict);
        let _ = execute!(stdout, ResetColor);
        if !is_waiting {
            println!();
            break;
        }
        last_verdict = verdict;
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    Ok(())
}

fn extract_verdict(verdict: &String) -> (String, bool) {
    let is_accepted = verdict.contains("<span class=\"verdict-accepted\">");
    match verdict.find("<span class=\"verdict-") {
        None => (verdict.trim().to_string(), false),
        Some(pos) => {
            let rem = &verdict[pos..];
            let pos1 = match rem.find(">") {
                None => return (verdict.clone(), false),
                Some(pos) => pos + 1,
            };
            let rem1 = &rem[pos1..];
            let rem2 = rem1.replace("<span class=\"verdict-format-judged\">", "");
            let pos2 = match rem2.find("</span>") {
                None => return (verdict.clone(), false),
                Some(pos) => pos,
            };
            (rem2[..pos2].to_string(), is_accepted)
        }
    }
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
