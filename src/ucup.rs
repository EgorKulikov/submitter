use crate::uoj::UojClient;

pub fn login() {
    let mut client = UojClient::new("https://contest.ucup.ac", "Universal Cup");
    if let Err(e) = client.login() {
        eprintln!("Login failed: {}", e);
    }
}

pub fn submit(url: String, language: String, source: String) {
    let mut client = UojClient::new("https://contest.ucup.ac", "Universal Cup");

    println!("Logging in");
    if let Err(e) = client.login() {
        eprintln!("Login failed: {}", e);
        return;
    }

    // Extract the problem path from the full URL
    // e.g. https://contest.ucup.ac/contest/1106/problem/A -> /contest/1106/problem/A
    let path = match url.find("contest.ucup.ac") {
        Some(pos) => &url[pos + "contest.ucup.ac".len()..],
        None => {
            eprintln!("Bad URL: {}", url);
            return;
        }
    };

    println!("Submitting");
    if let Err(e) = client.submit(path, &language, &source) {
        eprintln!("Submit failed: {}", e);
    }
}
