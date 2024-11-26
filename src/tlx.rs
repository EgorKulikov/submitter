use dialoguer::console::Term;
use dialoguer::{Input, Password};
use std::collections::HashMap;
use std::fs::{read_to_string, write};
use std::path::Path;
use thirtyfour::error::WebDriverResult;
use thirtyfour::{By, Cookie, WebDriver};

pub async fn login(driver: &WebDriver, cookies: Vec<Cookie>) -> WebDriverResult<Vec<Cookie>> {
    driver.goto("https://tlx.toki.id/").await?;
    driver.delete_all_cookies().await?;
    for cookie in cookies {
        driver.add_cookie(cookie).await?;
    }
    let storage = read_to_string("tlx_storage.json").unwrap_or("".to_string());
    if !storage.is_empty() {
        driver.execute("window.localStorage.clear();", vec![]).await?;
        let items: HashMap<String, String> = serde_json::from_str(&storage).unwrap();
        eprintln!("Setting session");
        for (key, value) in items {
            driver.execute(format!("window.localStorage.setItem('{}', '{}');", key, value), vec![]).await?;
        }
        let value = driver.execute("return window.localStorage.getItem('persist:session');", vec![]).await?;
        let value = value.convert::<String>()?;
        eprintln!("{}", value);
    }
    driver.goto("https://tlx.toki.id/login").await?;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    driver.screenshot(&Path::new("tlx.png")).await?;
    let value = driver.execute("return window.localStorage.getItem('persist:session');", vec![]).await?;
    let value = value.convert::<String>()?;
    eprintln!("{}", value);
    if driver.current_url().await?.as_str() != "https://tlx.toki.id/login" {
        return Ok(driver.get_all_cookies().await?);
    }
    let login: String = Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Enter your tlx login")
        .interact_on(&Term::stdout())
        .unwrap();
    let password: String = Password::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Enter your tlx password")
        .interact_on(&Term::stdout())
        .unwrap();
    driver.find(By::Name("usernameOrEmail")).await?.send_keys(login).await?;
    driver.find(By::Name("password")).await?.send_keys(password).await?;
    driver.find(By::ClassName("bp5-button")).await?.click().await?;
    driver.screenshot(&Path::new("tlx_after.png")).await?;
    // #[derive(Debug)]
    // struct GetStorage {
    //     key: String,
    // }
    // impl ExtensionCommand for GetStorage {
    //     fn parameters_json(&self) -> Option<Value> {
    //         Some(json!(self.key))
    //     }
    //
    //     fn method(&self) -> Method {
    //         Method::GET
    //     }
    //
    //     fn endpoint(&self) -> Arc<str> {
    //         Arc::from("/localStorage")
    //     }
    // }
    // eprintln!("{:?}", driver.cmd(Command::ExtensionCommand(Box::new(GetStorage { key: "persist:session".to_string() }))).await?);
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    let value = driver.execute("var ls = window.localStorage, items = {};  \
                               for (var i = 0, k; i < ls.length; ++i)  \
                                 items[k = ls.key(i)] = ls.getItem(k);  \
                               return items; ", vec![]).await?;
    let value = value.json().to_string();
    write("tlx_storage.json", value).unwrap();
    Ok(driver.get_all_cookies().await?)
}

pub async fn submit(driver: &WebDriver, url: String, _language: String, source: String) -> WebDriverResult<()> {
    Ok(())
}
