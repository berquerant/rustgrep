use clap::Parser;
use regex::Regex;
use rustgrep::grep::{Grep, Pool};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[clap(version = "0.1.0", author = "berquerant")]
struct Opts {
    /// Number of grep threads.
    #[clap(short = 'j', default_value = "1")]
    thread_num: usize,
    /// Regex string, required.
    regex: String,
    /// Grep target files.
    files: Vec<PathBuf>,
}

fn main() {
    let opts = Opts::parse();
    if opts.thread_num < 1 {
        panic!("non-positive thread_num");
    }
    let r = Regex::new(&opts.regex).expect("compile regex");
    match opts.files.len() {
        0 => {
            let (pool, recv) = Pool::new(r, opts.thread_num);
            Grep::grep_stdin(pool, recv)
        }
        1 => {
            let (pool, recv) = Pool::new(r, opts.thread_num);
            Grep::grep_file(pool, recv, opts.files[0].clone())
        }
        _ => Grep::grep_files(r, opts.thread_num, opts.files),
    }
}
