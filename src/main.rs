// SPDX-License-Identifier: Apache-2.0

use clap::{App, Arg};
use std::io::prelude::*;
use std::io::{BufReader, ErrorKind, Read, Result, Write};
use std::path::{Path, PathBuf};
use wasmparser::{Chunk, Parser, Payload::*};

const RESOURCES_SECTION: &str = ".enarx.resources";

fn read_paths(reader: &mut impl Read) -> Result<Vec<PathBuf>> {
    let mut reader = BufReader::new(reader);
    let mut result: Vec<PathBuf> = Vec::new();

    loop {
        let mut buf = String::new();
        let size = reader.read_line(&mut buf)?;
        if size == 0 {
            break;
        }

        let path = buf.trim_end().into();
        result.push(path);
    }

    Ok(result)
}

fn create_archive(paths: Vec<PathBuf>, prefix: &str, writer: &mut impl Write) -> Result<()> {
    let mut builder = tar::Builder::new(writer);

    for path in paths {
        for ancestor in path.ancestors() {
            if ancestor == Path::new("") {
                break;
            }
            let metadata = std::fs::metadata(&ancestor)?;
            if !metadata.is_dir() && !metadata.is_file() {
                return Err(ErrorKind::InvalidInput.into());
            }
        }
        let name = path.strip_prefix(prefix).or(Err(ErrorKind::InvalidInput))?;
        builder.append_path_with_name(&path, &name)?;
    }

    builder.finish()?;

    Ok(())
}

fn filter(section: &str, mut input: impl Read, output: &mut impl Write) -> Result<()> {
    let mut buf = Vec::new();
    let mut parser = Parser::new(0);
    let mut eof = false;
    let mut stack = Vec::new();

    loop {
        let (payload, consumed) = match parser.parse(&buf, eof)
            .or(Err(ErrorKind::InvalidInput))?
        {
            Chunk::NeedMoreData(hint) => {
                assert!(!eof); // otherwise an error would be returned

                // Use the hint to preallocate more space, then read
                // some more data into our buffer.
                //
                // Note that the buffer management here is not ideal,
                // but it's compact enough to fit in an example!
                let len = buf.len();
                buf.extend((0..hint).map(|_| 0u8));
                let n = input.read(&mut buf[len..])?;
                buf.truncate(len + n);
                eof = n == 0;
                continue;
            }

            Chunk::Parsed { consumed, payload } => (payload, consumed),
        };

        match payload {
            CustomSection { name, .. } => {
                if name != section {
                    output.write_all(&buf[..consumed])?;
                }
            }
            // When parsing nested modules we need to switch which
            // `Parser` we're using.
            ModuleCodeSectionEntry { parser: subparser, .. } => {
                stack.push(parser);
                parser = subparser;
            }
            End => {
                if let Some(parent_parser) = stack.pop() {
                    parser = parent_parser;
                } else {
                    break;
                }
            }
            _ => {
                output.write_all(&buf[..consumed])?;
            }
        }

        // once we're done processing the payload we can forget the
        // original.
        buf.drain(..consumed);
    }
    Ok(())
}

fn append(section: &str, mut archive: &std::fs::File, writer: &mut impl Write) -> Result<()> {
    let mut header: Vec<u8> = Vec::new();
    let name = section.as_bytes();
    leb128::write::unsigned(&mut header, name.len() as u64)?;
    header.write_all(name)?;
    let size = archive.seek(std::io::SeekFrom::End(0))?;

    writer.write_all(&[0])?;
    leb128::write::unsigned(writer, size + header.len() as u64)?;
    writer.write_all(&header)?;

    let _ = archive.seek(std::io::SeekFrom::Start(0))?;
    loop {
        let mut buf = [0; 4096];
        let n = archive.read(&mut buf[..])?;

        if n == 0 {
            break;
        }

        writer.write_all(&buf[..n])?;
    }

    Ok(())
}

fn main() {
    let matches = App::new("wasm-bundle")
        .about("Bundle resource files into a Wasm file")
        .arg(
            Arg::with_name("INPUT")
                .help("Sets the input Wasm file")
                .required(true)
                .index(1),
        )
        .arg(
            Arg::with_name("OUTPUT")
                .help("Sets the output Wasm file")
                .required(true)
                .index(2),
        )
        .arg(
            Arg::with_name("prefix")
                .help("Sets the path prefix to be removed")
                .short("-p")
                .long("prefix")
                .takes_value(true)
                .default_value(""),
        )
        .arg(
            Arg::with_name("section")
                .help("Sets the section name")
                .short("-j")
                .long("section")
                .takes_value(true)
                .default_value(RESOURCES_SECTION),
        )
        .usage("find dir -type f | wasm-bundle INPUT OUTPUT")
        .get_matches();

    let input_path = matches.value_of("INPUT").unwrap();
    let output_path = matches.value_of("OUTPUT").unwrap();

    // Create tar archive from the file list read
    let mut reader = std::io::stdin();
    let paths = read_paths(&mut reader).expect("couldn't read file list");
    let mut archive = tempfile::tempfile().expect("couldn't create a temp file");

    let prefix = matches.value_of("prefix").unwrap();
    create_archive(paths, &prefix, &mut archive).expect("couldn't create archive");

    // Filter out the existing .resources section
    let input = std::fs::read(&input_path).expect("couldn't open input file");
    let mut output = std::fs::File::create(&output_path).expect("couldn't create output file");

    let section = matches.value_of("section").unwrap();
    filter(&section, input.as_slice(), &mut output).expect("couldn't filter sections");

    // Append a custom .resources section with the created archive
    append(&section, &archive, &mut output).expect("couldn't append custom section");
}
