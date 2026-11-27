pub mod atcoder;
pub mod codechef;
pub mod coderun;
pub mod kattis;
pub mod kep;
pub mod langmatch;
pub mod luogu;
pub mod new_yandex;
pub mod yosupo;
pub mod codeforces;
pub mod doj;
pub mod repovive;
pub mod eolymp;
pub mod extbridge;
pub mod http;
pub mod toph;
pub mod ucup;
pub mod uoj;
pub mod yandex;

pub fn clear(len: usize) {
    for _ in 0..len {
        print!("{}", 8u8 as char);
    }
    for _ in 0..len {
        print!(" ");
    }
    for _ in 0..len {
        print!("{}", 8u8 as char);
    }
}

/// Reads the cookie JSON a user pastes at a login prompt, line by line until
/// its brackets balance, and returns the text read. `site` is the short name
/// `submitter login` takes, for the hint shown when nothing can be read.
///
/// End of input before any JSON is an error: it is what reading gives when
/// submitter runs without a terminal (from an editor or a script), where
/// nothing can ever be pasted, so asking again would loop forever.
pub fn read_pasted_json<R: std::io::BufRead>(mut reader: R, site: &str) -> Result<String, String> {
    let mut input = String::new();
    let mut bracket_count = 0i32;
    loop {
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .map_err(|e| format!("Failed to read input: {}", e))?;
        if read == 0 {
            if input.trim().is_empty() {
                return Err(format!(
                    "No cookies were pasted, input is closed. Log in from a terminal with `submitter login {}`",
                    site
                ));
            }
            // A paste cut short: the JSON parser reports what is missing.
            return Ok(input);
        }
        for c in line.chars() {
            if c == '[' || c == '{' {
                bracket_count += 1;
            } else if c == ']' || c == '}' {
                bracket_count -= 1;
            }
        }
        input.push_str(&line);
        if bracket_count <= 0 && !input.trim().is_empty() {
            return Ok(input);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::read_pasted_json;

    #[test]
    fn reads_json_across_lines() {
        let pasted = "\n[\n  {\"name\": \"a\", \"value\": \"1\"}\n]\nleft unread\n";
        let json = read_pasted_json(pasted.as_bytes(), "atcoder").unwrap();
        assert_eq!(json.trim(), "[\n  {\"name\": \"a\", \"value\": \"1\"}\n]");
    }

    #[test]
    fn closed_input_is_an_error() {
        let err = read_pasted_json("".as_bytes(), "atcoder").unwrap_err();
        assert!(err.contains("submitter login atcoder"), "{}", err);
        let err = read_pasted_json("\n  \n".as_bytes(), "atcoder").unwrap_err();
        assert!(err.contains("submitter login atcoder"), "{}", err);
    }

    #[test]
    fn cut_short_paste_is_returned() {
        let json = read_pasted_json("[{\"name\":".as_bytes(), "atcoder").unwrap();
        assert_eq!(json, "[{\"name\":");
    }
}
