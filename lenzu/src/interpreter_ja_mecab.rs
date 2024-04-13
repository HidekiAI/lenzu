//extern crate mecab;
use crate::interpreter_traits::{InterpreterTrait, InterpreterTraitResult}; // so odd that unless I'd  import it in main.rs, this will not be recognized, but once it is recognized, you can comment it in main.rs
use anyhow::Error;
use encoding_rs::{Decoder, Encoding};
use mecab::Model;
use std::{
    collections::HashMap,
    hash::Hash,
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread::JoinHandle,
};
use winapi::um::commctrl::TTM_UPDATETIPTEXTA;

pub(crate) struct InterpreterJaMecab {}

impl InterpreterTrait for InterpreterJaMecab {
    fn new() -> Self
    where
        Self: Sized,
    {
        InterpreterJaMecab {}
    }

    fn init(&self) -> Vec<String> {
        vec!["ja".to_string(), "en".to_string()]
    }

    fn convert(&self, lines_utf8: &Vec<String>) -> Result<InterpreterTraitResult, Error> {
        // make sure we do not have any illegal characters
        for line in lines_utf8.iter() {
            if line.contains('\u{0}') || line.contains(' ') || line.contains(" ") {
                return Err(anyhow::anyhow!("Illegal character found in text"));
            }
        }
        let start_timer = std::time::Instant::now();
        let shared_lines: Arc<
            Mutex<
                HashMap<
                    usize, /*thread index*/
                    HashMap<usize /*newline_index*/, Vec<String> /*words-sequential */>,
                >,
            >,
        > = Arc::new(Mutex::new(HashMap::new())); // store/write with MUTEX
        let mut handles: Vec<JoinHandle<()>> = vec![]; // since it's OUTSIDE the spawn lambda, it's not shared between threads
        let fn_upsert_line =
            |map_line: &mut HashMap<usize /*line_index*/, Vec<String> /*words*/>,
             line_index: usize,
             new_word: String| {
                match map_line.get_mut(&line_index) {
                    Some(line) => {
                        line.push(new_word);
                    }
                    None => {
                        map_line.insert(line_index, vec![new_word]);
                    }
                }
            };
        // prepopulate with empty HashMaps so one just needs to "update" based on each thread_index
        for thread_index in 0..lines_utf8.len() {
            shared_lines
                .lock()
                .unwrap()
                .insert(thread_index, HashMap::new());
        }

        // NOTE: this is single-threaded, so there are no outer loop for each thread to spawn...  Just ONE spawn...
        let thread_count = lines_utf8.len();
        for thread_index in 0..thread_count {
            let lines_mutex_per_thread = shared_lines.clone(); // PER thread, we need to clone the MUTEX so that we can lock on THIS thread to write to shared vector
            let shared_text_per_thread = lines_utf8[thread_index].clone();
            let model = Arc::new(Model::new(""));
            let handle = std::thread::spawn(move || {
                // create tagger based on the model
                let tagger = model.create_tagger();

                // create lattice object per thread
                let mut lattice = model.create_lattice();

                // get tagged result as string
                lattice.set_sentence(shared_text_per_thread);

                // parse lattice
                tagger.parse(&lattice);

                // space separate the morphemes iterated over in the lattice BOS (beginning of sentence) node
                let mut line = String::new();
                let mut is_begin = true;
                let mut is_eos = false;
                let mut line_index = 0;
                for node in lattice.bos_node().iter_next() {
                    //$ mecab --output-format-type="" --node-format="%f[8][%f[6]] " <<< "すもももももももものうち。最近人気のデスクトップなリナックスです!"
                    let features: Vec<&str> = node.feature.split(',').collect();
                    let original_surface_tokens = &(node.surface)[..(node.length as usize)];
                    if cfg!(debug_assertions) {
                        // NOTE: this println will cost you ~4X (i.e. 2mS evaluation will turn into 8.75mS)
                        println!(
                            "{}: {}\t{:?}",
                            thread_index, original_surface_tokens, features
                        );
                    }
                    // Skip BOS
                    if is_begin && features[0] == "BOS/EOS" {
                        is_begin = false;
                        continue;
                    }
                    // check for EOS
                    is_eos = features[0] == "BOS/EOS" && is_begin == false;
                    if features.len() < 10 {
                        continue;
                    }
                    // using `--node-format="%f[8][%f[6]] " `
                    let original = features[6];
                    let reading = features[8];
                    // if it's "。", "\n", or EOS, then flush and make new line
                    if is_eos || original == "。" || original == "\n" {
                        if original != "。" {
                            line += original;
                        }
                        let binding = lines_mutex_per_thread.lock().unwrap();
                        let mut map = binding.get(&thread_index).unwrap().clone();
                        fn_upsert_line(&mut map, line_index, line);
                        lines_mutex_per_thread
                            .lock()
                            .unwrap()
                            .insert(thread_index, map); // upsert

                        line = String::new();
                        line_index += 1;
                        continue; // not breaking, NOTE: It seems you can have multiple BOS...EOSBOS...EOS in a line...
                    }

                    // we only want the original and reading, but will discard the reading if it is the same as original
                    let possible_reading = if original == reading || reading == "*" || reading == ""
                    {
                        None
                    } else {
                        Some(reading)
                    };
                    match possible_reading {
                        Some(reading) => {
                            line += format!("{}[{}] ", original, reading).as_str();
                        }
                        None => {
                            line += format!("{} ", original).as_str();
                        }
                    }
                } // for

                // and finally, flush the last line
                if line != "" {
                    //lines_mutex_per_thread
                    //    .lock()
                    //    .unwrap()
                    //    .push((thread_index, line));
                    let binding = lines_mutex_per_thread.lock().unwrap();
                    let mut map = binding.get(&thread_index).unwrap().clone();
                    fn_upsert_line(&mut map, line_index, line);
                    lines_mutex_per_thread
                        .lock()
                        .unwrap()
                        .insert(thread_index, map); // upsert
                }
            });
            handles.push(handle);
        } // for

        // wait for all threads to finish (unordered)
        for handle in handles {
            handle.join().unwrap();
        }
        let elapsed = start_timer.elapsed();
        println!("Elapsed: {:?} ({} seconds)", elapsed, elapsed.as_secs_f64());

        // finally, marshal the lines into readonly Vec<String>
        let ret_lines_marshled: Vec<String> = {
            let map_of_map: HashMap<usize, HashMap<usize, Vec<String>>> =
                shared_lines.lock().unwrap().clone();
            println!("binding: {:?}", map_of_map.clone());

            let mut map_of_map_as_vec: Vec<(usize, Vec<(usize, Vec<String>)>)> = map_of_map
                .iter()
                .map(|(thread_index, inner_map)| {
                    let vec: Vec<(usize, Vec<String>)> = inner_map
                        .clone()
                        .iter()
                        .map(|(line_index, words)| (line_index.clone(), words.clone()))
                        .collect();
                    (thread_index.clone(), vec)
                })
                .collect();

            // we can now sort/order by thread_index and assume that the line_index is already ordered
            // in which we can then remove the thread_index and return the Vec<Vec<String>>
            map_of_map_as_vec.sort_by(|tup1, tup2| tup1.0.cmp(&tup2.0));
            map_of_map_as_vec = map_of_map_as_vec
                .iter()
                .map(|tup| {
                    let mut m_vec = tup.1.clone();
                    m_vec.sort_by(|tup1, tup2| tup1.0.cmp(&tup2.0));
                    (tup.0, m_vec)
                })
                .collect::<Vec<(usize, Vec<(usize, Vec<String>)>)>>();

            let lines_sequential: Vec<Vec<String>> = map_of_map_as_vec
                .iter()
                .map(|tup| {
                    let mut ret_lines: Vec<String> = vec![];
                    for (_, words) in tup.1.iter() {
                        ret_lines.push(words.join(" "));
                    }
                    ret_lines
                })
                .collect::<Vec<Vec<String>>>();

            // now, sort by line_index
            let mut ret_lines_marshled: Vec<String> = lines_sequential
                .iter()
                .flatten()
                .map(|line| line.clone())
                .collect();
            ret_lines_marshled
        };
        Ok(InterpreterTraitResult {
            text: ret_lines_marshled.join("..."),
            lines: ret_lines_marshled,
        })
    }
}

impl InterpreterJaMecab {
    pub fn new() -> Self {
        InterpreterJaMecab {}
    }
}

#[cfg(test)]
mod tests {
    use std::thread::{spawn, JoinHandle};

    use super::*;

    #[test]
    fn test_shared_vec() {
        let shared_array_mutexed: Arc<Mutex<Vec<i32>>> = Arc::new(Mutex::new(vec![]));
        let mut handles: Vec<JoinHandle<()>> = vec![]; // since it's OUTSIDE the spawn lambda, it's not shared between threads
        let threads = 5;

        for thread_index in 0..threads {
            let my_array_mutex = shared_array_mutexed.clone();

            let handle = spawn(move || {
                let mut shared = my_array_mutex.lock().unwrap();
                shared.push(thread_index);
            });
            handles.push(handle);
        }
        // now wait to join all threads
        for handle in handles {
            handle.join().unwrap();
        }

        // calling clone() on a Mutex<T> returns the T inside the Mutex
        let final_thread_iters = shared_array_mutexed.lock().unwrap().clone();
        println!("Final: {:?}", final_thread_iters); // OUTPUT: [0, 4, 2, 1, 3] - not guaranteed to be in order!
    }
}
