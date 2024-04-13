extern crate mecab;

use mecab::Model;
use std::{
    sync::{Arc, Mutex},
    thread::{spawn, JoinHandle},
};

// see https://github.com/taku910/mecab/blob/master/mecab/example/example.c
fn main() {
    let start_timer = std::time::Instant::now();
    let shared_lines: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![])); // store/write with MUTEX

    // OUTPUT: すもも[スモモ] も[モ] もも[モモ] も[モ] もも[モモ] の[ノ] うち[ウチ] 。 最近[サイキン] 人気[ニンキ] の[ノ] デスク トップ だ[ナ] です[デス] !
    let input = "すもももももももものうち。最近人気のデスクトップなリナックスです!";

    // NOTE: this is single-threaded, so there are no outer loop for each thread to spawn...  Just ONE spawn...
    let thread_count = 1;
    let mut handles: Vec<JoinHandle<()>> = vec![];
    for _ in 0..thread_count {
        let lines_mutex_per_thread = shared_lines.clone(); // PER thread, we need to clone the MUTEX so that we can lock on THIS thread to write to shared vector

        // create model object
        let model = Arc::new(Model::new(""));

        let handle = spawn(move || {
            // create tagger based on the model
            let tagger = model.create_tagger();

            // create lattice object per thread
            let mut lattice = model.create_lattice();

            // get tagged result as string
            lattice.set_sentence(input);

            // parse lattice
            tagger.parse(&lattice);

            //// get N best results
            //lattice.set_request_type(mecab::MECAB_NBEST);
            //lattice.set_sentence(input);
            //tagger.parse(&lattice);

            //// marginal probabilities
            //lattice.remove_request_type(mecab::MECAB_NBEST);
            //lattice.set_request_type(mecab::MECAB_MARGINAL_PROB);
            //lattice.set_sentence(input);
            //tagger.parse(&lattice);

            // space separate the morphemes iterated over in the lattice BOS (beginning of sentence) node
            let mut line = String::new();
            for node in lattice.bos_node().iter_next() {
                // NOTE that format differs between mecab offered in distro package  (i.e. Debian does not have -Oyomi while MSYS2 does - use `mecab --dump-config` to evaluate...)
                // 表層形\t品詞,品詞細分類1,品詞細分類2,品詞細分類3,活用型,活用形,原形,読み,発音
                // e.g.  '太郎' -> [["名詞", "固有名詞", "人名", "名", "*", "*", "太郎", "タロウ", "タロー", "", ""]] [1]
                // 1. Surface form of the morpheme - e.g. 太郎
                // 2. Part of speech - e.g. 名詞 (noun) or 動詞 (verb) or 助詞 (particle) etc.
                // 3. Subclass1 of part of speech - e.g. 固有名詞 (proper noun) or 自立 (independent) etc.
                // 4. Subclass2 of part of speech - e.g. 人名 (person's name) or 一般 (general) etc.
                // 5. Subclass3 of part of speech - e.g. 名 (name)  etc.
                // 6. Inflection type - e.g. 子音動詞ラ行 (consonant verb type 5) etc.
                // 7. Inflection form - e.g. 基本形 (base form) etc.
                // 8. Original form - e.g. 太郎 etc.
                // 9. Reading - e.g. タロウ etc.
                // 10. Pronunciation - e.g. タロー etc.
                // for more output format details, see: https://taku910.github.io/mecab/format.html
                // On some distros (i.e. MSYS), using `--node-format` option is possible:
                //      $ mecab --node-format="%f[7] "  <<< "太郎は次郎が持っている本を花子に渡した。"
                //      タロウ ハ ジロウ ガ モッ テ イル ホン ヲ ハナ コ ニ ワタシ タ 。 EOS

                // On some Debian distro (I'm unsure why on Debian acts like MSYS and another same Debian version acts differently),
                // but the following format is what this prototype is based on:
                //      $ mecab --output-format-type="" --node-format="%f[8][%f[6]] " <<< "すもももももももものうち。最近人気のデスクトップなリナックスです!"
                //      すもも[スモモ] も[モ] もも[モモ] も[モ] もも[モモ] の[ノ] うち[ウチ] 。[] 最近[サイキン] 人気[ニンキ] の[ノ] デスクトップ[デスクトップ] な[ダ] リナックス[リナックス] です[デス] ![] EOS
                let features: Vec<&str> = node.feature.split(',').collect();
                let original_surface_tokens = &(node.surface)[..(node.length as usize)];
                let is_eos = features[0] == "BOS/EOS";
                println!("{}\t{:?}", original_surface_tokens, features);
                if features.len() < 10 {
                    continue;
                }

                // using `--node-format="%f[8][%f[6]] " `
                let original = features[6];
                let reading = features[8];
                // if it's "。", "\n", or EOS, then flush and make new line
                if original == "。" || original == "\n" || is_eos {
                    if original != "。" {
                        lines_mutex_per_thread.lock().unwrap().push(line);
                        line = String::new();
                        continue;
                    }
                    // else
                    line += original;
                    lines_mutex_per_thread.lock().unwrap().push(line);
                    line = String::new();
                    continue;
                }

                // we only want the original and reading, but will discard the reading if it is the same as original
                let possible_reading = if original == reading || reading == "*" || reading == "" {
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
            }
            // and finally, flush the last line
            if line != "" {
                lines_mutex_per_thread.lock().unwrap().push(line);
            }
        });
        //handle.join().unwrap(); // sync/block/wait for completion
        handles.push(handle);
    }
    for handle in handles {
        handle.join().unwrap();
    }
    let elapsed = start_timer.elapsed();
    println!("Elapsed: {:?} ({} seconds)", elapsed, elapsed.as_secs_f64());

    for line in shared_lines.lock().unwrap().iter() {
        println!("{}", line);
    }
}

#[cfg(test)]
mod tests {
    use std::thread::JoinHandle;

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
