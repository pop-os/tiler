// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

use pop_tiler::TCellOwner;
use pop_tiler_service::Service;
use std::io::{BufRead, BufReader, Write};
fn main() {
    let input = std::io::stdin();
    let mut input = BufReader::new(input.lock()).lines();

    let output = std::io::stdout();
    let mut output = output.lock();

    let mut t = TCellOwner::<()>::new();
    let mut tiler = Service::default();

    while let Some(Ok(line)) = input.next() {
        match serde_json::from_str(&line) {
            Ok(request) => {
                for event in tiler.handle(request, &mut t) {
                    match serde_json::to_string(&event) {
                        Ok(mut string) => {
                            string.push('\n');
                            let _ = output.write_all(string.as_bytes());
                        }
                        Err(why) => {
                            eprintln!("pop-tiler-ipc: failed to serialize response: {}", why);
                        }
                    }
                }
            }
            Err(why) => {
                eprintln!("pop-tiler-ipc: failed to read from stdin: {}", why);
            }
        }
    }
}
