use regex::Regex;
use std::fs::File;
use std::io::{self, BufRead};
use std::iter::Iterator;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

const GREP_BUFFER_CHUNK_SIZE: usize = 100;

pub struct Grep;

impl Grep {
    pub fn grep_stdin(pool: Pool, recv: ResponseReceiver) {
        let t = thread::spawn(move || {
            for res in recv {
                println!("{}", res.0);
            }
        });

        let stdin = io::stdin();
        let mut buf = Vec::new();
        for line in stdin.lock().lines() {
            if buf.len() >= GREP_BUFFER_CHUNK_SIZE {
                pool.send(buf.clone());
                buf.clear();
            }
            let line = line.unwrap();
            buf.push(line.trim_end().to_string());
        }
        if !buf.is_empty() {
            pool.send(buf);
        }
        drop(pool);
        t.join().unwrap();
    }

    pub fn grep_file(pool: Pool, recv: ResponseReceiver, file: PathBuf) {
        let t = thread::spawn(move || {
            for res in recv {
                println!("{}", res.0);
            }
        });

        let file = File::open(file).unwrap();
        let buf = io::BufReader::new(file);
        {
            let chunks = BufReadChunk::new(Box::new(buf));
            for r in chunks {
                let chunk = r.unwrap();
                pool.send(chunk);
            }
        }
        drop(pool);
        t.join().unwrap();
    }

    pub fn grep_files(r: regex::Regex, n: usize, files: Vec<PathBuf>) {
        for file in files {
            let (pool, recv) = Pool::new(r.clone(), n);
            let path = file.clone().into_os_string().into_string().unwrap();
            let t = thread::spawn(move || {
                for res in recv {
                    println!("{}:{}", path, res.0);
                }
            });

            let f = File::open(file).unwrap();
            let buf = io::BufReader::new(f);
            {
                let chunks = BufReadChunk::new(Box::new(buf));
                for r in chunks {
                    let chunk = r.unwrap();
                    pool.send(chunk);
                }
            }
            drop(pool);
            t.join().unwrap();
        }
    }
}

pub type Chunk = Vec<String>;

/// Separate lines into chunks.
struct BufReadChunk {
    buf: Box<dyn BufRead>,
}

impl BufReadChunk {
    pub fn new(buf: Box<dyn BufRead>) -> BufReadChunk {
        BufReadChunk { buf }
    }
}

impl Iterator for BufReadChunk {
    type Item = io::Result<Chunk>;

    fn next(&mut self) -> Option<io::Result<Chunk>> {
        let mut v = Vec::new();
        while v.len() < GREP_BUFFER_CHUNK_SIZE {
            let mut s = String::new();
            match self.buf.read_line(&mut s) {
                Ok(0) => return if v.is_empty() { None } else { Some(Ok(v)) },
                Ok(_) => v.push(s.trim_end().to_string()),
                Err(x) => return Some(Err(x)),
            }
        }
        Some(Ok(v))
    }
}

/// A request message for Worker.
enum Request {
    /// New lines to be grepped.
    Lines(Chunk),
    /// Poison pill of Worker.
    Shutdown,
}
#[derive(Debug)]
pub struct Response(String);
type RequestSender = mpsc::Sender<Request>;
type RequestReceiver = mpsc::Receiver<Request>;
type ResponseSender = mpsc::Sender<Response>;
pub type ResponseReceiver = mpsc::Receiver<Response>;

pub struct Pool {
    workers: Vec<Worker>,
    sender: RequestSender,
}

impl Pool {
    /// Return a new worker pool and a channel to receive response.
    pub fn new(r: Regex, size: usize) -> (Pool, ResponseReceiver) {
        assert!(size > 0);
        // pool.send() => req_send
        // => worker {
        //     req_recv => res_send
        // } => res_recv
        let (req_send, req_recv) = mpsc::channel();
        let req_recv = Arc::new(Mutex::new(req_recv));
        let (res_send, res_recv) = mpsc::channel();
        let mut workers = Vec::with_capacity(size);
        for _ in 0..size {
            workers.push(Worker::new(
                r.clone(),
                Arc::clone(&req_recv),
                res_send.clone(),
            ));
        }
        (
            Pool {
                sender: req_send,
                workers,
            },
            res_recv,
        )
    }

    /// Send a new lines to be grepped.
    pub fn send(&self, r: Chunk) {
        self.sender.send(Request::Lines(r)).unwrap();
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        for _ in &self.workers {
            // shutdown all workers
            self.sender.send(Request::Shutdown).unwrap();
        }
        for w in &mut self.workers {
            // join all
            if let Some(t) = w.t.take() {
                t.join().unwrap();
            }
        }
    }
}

/// A worker thread for grep.
struct Worker {
    t: Option<thread::JoinHandle<()>>,
}

impl Worker {
    /// Return a new worker and start consuming messages from receiver.
    /// Send matched lines to sender.
    fn new(r: Regex, receiver: Arc<Mutex<RequestReceiver>>, sender: ResponseSender) -> Worker {
        let t = thread::spawn(move || {
            for req in receiver.lock().unwrap().iter() {
                match req {
                    Request::Shutdown => {
                        break;
                    }
                    Request::Lines(lines) => {
                        for line in lines {
                            if r.is_match(&line) {
                                sender.send(Response(line)).unwrap();
                            }
                        }
                    }
                }
            }
        });
        Worker { t: Some(t) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! test_pool {
        ($name:ident, $r:expr, $n:expr, $lines_list:expr, $want:expr) => {
            #[test]
            fn $name() {
                let rx = Regex::new($r).unwrap();
                let (p, recv) = Pool::new(rx, $n);
                for lines in $lines_list {
                    p.send(lines);
                }
                drop(p);
                let mut got = recv.iter().map(|x| x.0).collect::<Vec<_>>();
                let mut want = $want;
                want.sort();
                got.sort();
                assert_eq!(want, got);
            }
        };
    }

    macro_rules! strs2vec {
        ($v:expr) => {
            $v.iter().map(|x| x.to_string()).collect::<Vec<_>>()
        };
    }

    fn repeat(n: usize, mut v: Vec<String>) -> Vec<String> {
        for _ in 1..n {
            let u = v.clone();
            v.extend(u);
        }
        v
    }

    test_pool!(
        pool_not_matched,
        "mesosphere",
        1,
        vec![
            strs2vec!(["pow", "wor"]),
            strs2vec!(["ld", "er"]),
            strs2vec!(["whole", "power"])
        ],
        Vec::<String>::new()
    );

    test_pool!(
        pool_matched,
        "pow",
        1,
        vec![
            strs2vec!(["pow", "wor"]),
            strs2vec!(["ld", "er"]),
            strs2vec!(["whole", "power"])
        ],
        strs2vec!(["pow", "power"])
    );

    test_pool!(
        pool_not_matched_concurrently,
        "mesosphere",
        2,
        vec![
            repeat(10, strs2vec!(["pow", "wor"])),
            repeat(10, strs2vec!(["ld", "er"])),
            repeat(10, strs2vec!(["whole", "power"]))
        ],
        Vec::<String>::new()
    );

    test_pool!(
        pool_matched_concurrently,
        "pow",
        2,
        vec![
            repeat(10, strs2vec!(["pow", "wor"])),
            repeat(10, strs2vec!(["ld", "er"])),
            repeat(10, strs2vec!(["whole", "power"]))
        ],
        repeat(10, strs2vec!(["pow", "power"]))
    );

    #[test]
    fn worker_should_run() {
        let (req_send, req_recv) = mpsc::channel();
        let req_recv = Arc::new(Mutex::new(req_recv));
        let (res_send, res_recv) = mpsc::channel();
        let r = Regex::new("pow").unwrap();
        let mut _worker = Worker::new(r, req_recv, res_send);
        req_send
            .send(Request::Lines(
                ["pow", "wor"].iter().map(|x| x.to_string()).collect(),
            ))
            .unwrap();
        req_send.send(Request::Shutdown).unwrap();
        let got: Vec<String> = res_recv.iter().map(|x| x.0).collect::<Vec<_>>();
        assert_eq!(vec!["pow".to_string()], got);
    }
}
