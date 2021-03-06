extern crate getopts;
extern crate hex;

use std::env;
use std::fs::{OpenOptions, File};
use std::io;
use std::io::prelude::*;
use std::io::SeekFrom;
use std::path::Path;

use getopts::Options;
use sha2::{Sha256, Digest};
use sha2::digest::generic_array::GenericArray;
use sha2::digest::generic_array::typenum::U32;

const KB: u64 = 1024;
const DEFAULT_BUF_SIZE: usize = 1024;
const BLOCK_SIZE: usize = 1024;
const HASH_SIZE: usize = 32;

type HashVec = Vec<GenericArray<u8, U32>>;

#[derive(Debug)]
struct FileRevIter {
    file: File,
    filesize: u64,
    offset: i64,
}

impl FileRevIter {
    fn new<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let file = File::open(path)?;
        let metadata = file.metadata()?;
        let filesize = metadata.len();
        let offset = (filesize % KB) as i64;

        Ok(FileRevIter { file, filesize, offset })
    }
}

impl Iterator for FileRevIter {
    type Item = (usize, Vec<u8>);

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset <= self.filesize as i64 {
            self.file.seek(SeekFrom::End(-self.offset)).unwrap();

            let mut buf = vec![0; DEFAULT_BUF_SIZE];
            let len = self.file.read(&mut buf).unwrap();

            self.offset += 1024;

            return Some((len, buf));
        }
        None
    }
}

fn compute_hashes<P>(input_path: P, hashes: &mut HashVec) -> io::Result<()>
    where P: AsRef<Path>
{
    let file_iter = FileRevIter::new(input_path)?;

    // Iterates file from last block to first
    for (mut len, mut buf) in file_iter {
        if let Some(val) = hashes.last() {
            buf.extend(val);
            len = buf.len();
        }

        let hash = Sha256::digest(&buf[0..len]);
        hashes.push(hash);
    }

    Ok(())
}

fn sign<P>(input_path: P, output_path: P, hashes: &HashVec) -> io::Result<()>
    where P: AsRef<Path>
{
    let mut output_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)?;

    let mut input_file = File::open(input_path)?;
    let mut buf = vec![0; DEFAULT_BUF_SIZE];

    // We skip 1 because h0 is not included
    for h in hashes.iter().rev().skip(1) {
        // Write each block appended with the hash of the next block
        let len = input_file.read(&mut buf).unwrap();
        output_file.write(&buf[0..len]).unwrap();
        output_file.write(h).unwrap();
    }

    // Write last block (no appended hash)
    let len = input_file.read(&mut buf).unwrap();
    output_file.write(&buf[0..len]).unwrap();

    Ok(())
}

fn verify<P>(input_path: P, output_path: P, hash: &[u8]) -> io::Result<bool>
    where P: AsRef<Path>
{
    let mut input_file = File::open(input_path)?;
    let augmented_size = BLOCK_SIZE + HASH_SIZE;
    let mut buf = vec![0; augmented_size];
    let mut hash = GenericArray::clone_from_slice(hash);

    let mut output_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)?;

    loop {
        let len = input_file.read(&mut buf).unwrap();
        if len > 0 {
            let block_hash = Sha256::digest(&buf[0..len]);
            if hash != block_hash {
                return Ok(false);
            }
            if len != augmented_size {
                output_file.write(&buf[0..len]).unwrap();
                return Ok(true);
            }
            output_file.write(&buf[0..BLOCK_SIZE]).unwrap();
            hash = GenericArray::clone_from_slice(&buf[BLOCK_SIZE..]);
        } else {
            return Ok(false);
        }
    }
}

fn print_usage(opts: Options) {
    let brief = format!("Usage: ./target/debug/w3-file_auth \
        INPUT_FILE OUTPUT_FILE [options]");
    print!("{}", opts.usage(&brief));
}

fn main() -> io::Result<()> {
    let args: Vec<_> = env::args_os().skip(1).collect();

    let mut opts = Options::new();
    opts.optopt("v", "verify", "verify signed input file \
        and output original file", "HASH");
    opts.optflag("h", "help", "print this help menu");
    let matches = match opts.parse(&args) {
        Ok(m) => m,
        Err(f) => panic!(f.to_string()),
    };
    if matches.opt_present("h") {
        print_usage(opts);
        return Ok(());
    }
    let verify_hash = matches.opt_str("v");
    if matches.free.len() < 2 {
        print_usage(opts);
        return Ok(());
    }

    let input_filename = &args[0];
    let output_filename = &args[1];
    let input_path = Path::new(input_filename);
    let output_path = Path::new(output_filename);

    match verify_hash {
        Some(hash) => {
            let hash = hex::decode(hash).unwrap();
            let result = verify(&input_path, &output_path, &hash)?;
            println!("Verified: {}", result);
            if result {
                println!("File created: {}", output_path.display());
            }
        },
        None => {
            let mut hashes = Vec::new();
            compute_hashes(&input_path, &mut hashes)?;

            if let Some(val) = hashes.last() {
                println!("Hash 0: {:x}", val);
            }

            sign(&input_path, &output_path, &hashes)?;
            println!("File created: {}", output_path.display());
        },
    }

    Ok(())
}
