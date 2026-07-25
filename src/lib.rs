pub mod atcoder;
pub mod codechef;
pub mod coderun;
pub mod kattis;
pub mod kep;
pub mod langmatch;
pub mod luogu;
pub mod codeforces;
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
