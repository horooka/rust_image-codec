use aes::Aes128;
use cosmian_fpe::ff1::{BinaryNumeralString, FF1};
use image::{ImageBuffer, Rgb, imageops::dither};
use std::{
    fs,
    process::exit,
    sync::{Arc, Mutex},
    thread,
};

mod utils;
use utils::*;

fn encrypt(bytes: &mut [u8], key: &str) -> Option<()> {
    let byte_key = base64url_to_bytes(key)?;
    let ff1 = FF1::<Aes128>::new(&byte_key, 2).ok()?;
    let bn = BinaryNumeralString::from_bytes_le(bytes);
    let encrypted = ff1.encrypt(&[], &bn).ok()?;
    let encrypted_bytes = encrypted.to_bytes_le();
    bytes.copy_from_slice(&encrypted_bytes);
    Some(())
}

fn decrypt(cipher: &mut [u8], key: &str) -> Option<()> {
    let byte_key = base64url_to_bytes(key)?;
    let ff1 = FF1::<Aes128>::new(&byte_key, 2).ok()?;
    let bn = BinaryNumeralString::from_bytes_le(cipher);
    let decrypted = ff1.decrypt(&[], &bn).ok()?;
    let decrypted_bytes = decrypted.to_bytes_le();
    cipher.copy_from_slice(decrypted_bytes.as_slice());
    Some(())
}

fn process_encode(
    chunk: Vec<Rgb<u8>>,
    palette: &[Rgb<u8>],
    key_opt: Option<String>,
    progress_bar: Arc<Mutex<ProgressBar>>,
) -> Vec<u8> {
    let mut encode: Vec<u8> = Vec::with_capacity(chunk.len() / 3);
    for pixel in chunk {
        let r = pixel[0];
        let g = pixel[1];
        let b = pixel[2];

        let closest_index = palette
            .iter()
            .position(|&c| c[0] == r && c[1] == g && c[2] == b)
            .unwrap_or(0);
        encode.push(closest_index as u8);
        progress_bar.lock().unwrap().step();
    }

    if let Some(key) = key_opt {
        encrypt(&mut encode, key.as_str()).expect("Error: invalid code or key");
        progress_bar.lock().unwrap().step();
    }

    encode
}

fn process_decode(
    mut chunk: Vec<u8>,
    palette: &[Rgb<u8>],
    key_opt: Option<String>,
    progress_bar: Arc<Mutex<ProgressBar>>,
    cpus_amount: usize,
) -> Vec<u8> {
    if let Some(key) = key_opt {
        decrypt(&mut chunk, key.as_str()).expect("Error: invalid code or key");
        progress_bar
            .lock()
            .unwrap()
            .step_percent(1.0 / cpus_amount as f32);
    }
    let mut decode = Vec::with_capacity(chunk.len() * 3);
    for &byte in chunk.as_slice() {
        let rgb = palette.get(byte as usize).unwrap_or(&palette[0]);
        decode.push(rgb[0]);
        decode.push(rgb[1]);
        decode.push(rgb[2]);
        progress_bar.lock().unwrap().step();
    }
    decode
}

// Using result as enum for two "Ok()" dtypes
fn do_input(input: &str, encode: bool) -> Result<ImageBuffer<Rgb<u8>, Vec<u8>>, Vec<u8>> {
    if encode {
        return match open_img(input) {
            Ok(img) => Ok(img),
            Err(err) => {
                eprintln!("Error: {}", err);
                exit(1);
            }
        };
    }
    match fs::read(input) {
        Ok(bytes) => Err(bytes),
        Err(err) => {
            eprintln!("Error: {}", err);
            exit(1);
        }
    }
}

fn do_encode(
    mut img: ImageBuffer<Rgb<u8>, Vec<u8>>,
    palette_size: usize,
    key_opt: Option<String>,
    compress: bool,
) -> Vec<u8> {
    let pixels: Vec<Rgb<u8>> = img.pixels().cloned().collect();
    let (width, height) = img.dimensions();
    if !(2..=4097).contains(&width) {
        eprintln!("Error: width should be between 2 and 4097");
        exit(1);
    }
    if !(2..=4097).contains(&height) {
        eprintln!("Error: height should be between 2 and 4097");
        exit(1);
    }
    let palette = gen_palette(pixels.as_slice(), palette_size);
    dither(
        &mut img,
        &Palette {
            colors: palette.clone(),
        },
    );

    let cpus_amount = num_cpus::get();
    let data = Arc::new(img.pixels().cloned().collect::<Vec<Rgb<u8>>>());
    let bytes_per_thread = data.len().div_ceil(cpus_amount);
    let palette = Arc::new(palette);
    let progress_bar = Arc::new(Mutex::new(ProgressBar::new(data.len())));
    let mut handles = Vec::with_capacity(cpus_amount);
    for i in 0..cpus_amount {
        let data = Arc::clone(&data);
        let progress_bar = Arc::clone(&progress_bar);
        let palette = Arc::clone(&palette);
        let key_bind = key_opt.clone();
        let start = i * bytes_per_thread;
        let end = ((i + 1) * bytes_per_thread).min(data.len());

        let chunk = data[start..end].to_vec();
        let handle = thread::Builder::new()
            .name(format!("processing-{i}/{cpus_amount}"))
            .spawn(move || process_encode(chunk, &palette, key_bind, progress_bar))
            .unwrap();
        handles.push(handle);
    }
    let mut result = Vec::new();
    for handle in handles {
        let processed_chunk = handle.join().unwrap();
        result.extend(processed_chunk);
    }
    let palette_bytes = palette.iter().flat_map(|rgb| rgb.0).collect::<Vec<u8>>();
    let mut output_bytes = Vec::with_capacity(3 + palette_size * 3 + result.len());
    output_bytes.extend_from_slice(&pack_dimensions(width as u16 - 2, height as u16 - 2));
    output_bytes.push((palette_size - 2) as u8);
    output_bytes.extend_from_slice(&palette_bytes);
    output_bytes.extend_from_slice(&result);
    if compress {
        let compressed = zstd::encode_all(output_bytes.as_slice(), 0).expect("Compression failed");
        return if compressed.len() < output_bytes.len() {
            compressed
        } else {
            output_bytes
        };
    }
    output_bytes
}

fn do_decode(
    mut bytes: Vec<u8>,
    key_opt: Option<String>,
    compress: bool,
) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    if compress {
        let decompressed = zstd::decode_all(&mut bytes.as_slice()).expect("Decompression failed");
        bytes = decompressed;
    }
    let palette_size = bytes[3] as usize + 2;
    let palette = decode_palette(&bytes[4..(palette_size * 3) + 4]);
    let data = Arc::new(&bytes[(4 + palette.len() * 3)..]);
    let cpus_amount = num_cpus::get();
    let bytes_per_thread = data.len().div_ceil(cpus_amount);
    let mut handles = Vec::with_capacity(cpus_amount);
    let progress_bar = Arc::new(Mutex::new(ProgressBar::new(data.len())));
    for i in 0..cpus_amount {
        let data = Arc::clone(&data);
        let progress_bar = Arc::clone(&progress_bar);
        let palette_bind = palette.clone();
        let key_bind = key_opt.clone();

        let start = i * bytes_per_thread;
        let end = ((i + 1) * bytes_per_thread).min(data.len());
        let chunk: Vec<u8> = data[start..end].to_vec();
        let handle = thread::Builder::new()
            .name(format!("processing-{i}/{cpus_amount}"))
            .spawn(move || {
                process_decode(chunk, &palette_bind, key_bind, progress_bar, cpus_amount)
            })
            .unwrap();
        handles.push(handle);
    }
    let (width, height) = unpack_dimensions(&bytes[..=2]);
    let mut result = Vec::new();
    for handle in handles {
        let processed_chunk = handle.join().unwrap();
        result.extend(processed_chunk);
    }
    ImageBuffer::from_raw(width + 2, height + 2, result).expect(
        "Error: Not enough data. Image is compressed (add \"z\" flag to decode mode) or corrupted",
    )
}

// Using result as enum for two "Ok()" dtypes
fn do_output(data: Result<Vec<u8>, ImageBuffer<Rgb<u8>, Vec<u8>>>, output_file_path: &str) {
    match data {
        Ok(bytes) => {
            write_file(bytes.as_slice(), output_file_path);
        }
        Err(img) => {
            _ = save_img(img.clone(), output_file_path);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 1 {
        println!("Usage: exe [options] [input_file_path] [output_file_path] [palette_size(encode)] [base64url_key(optional)]

    options:
        - e - encode mode: input - existing [input_file_path], output - saved [output_file_path] or stderr
        - d - decode mode: input - existing [input_file_path], output - saved [output_file_path] or stderr
        - c - encryption-decryption flag
        - z - compression-decompression flag: requires additional [base64url_key] arg at last position
        - g - 16bytes base64url stdout key gen (doesn not need any input)");
        return;
    } else if args[1] == "g" {
        println!("{}", gen_key());
        return;
    } else if args[1] == "i" {
        println!("{}", get_info(args[2].as_str()));
        return;
    }
    let options = args[1].clone();
    let input_bytes = do_input(args[2].as_str(), options.contains("e"));
    let key = if options.contains("c") {
        if options.contains("e") {
            Some(args[5].clone())
        } else {
            Some(args[4].clone())
        }
    } else {
        None
    };

    // Using result as enum for two "Ok()" dtypes
    let processed_data = if options.contains("e") {
        let palette_size = args[4].parse::<usize>().unwrap();
        if !(2..=257).contains(&palette_size) {
            eprintln!("Error: palette size should be between 2 and 257");
            exit(1);
        }
        Ok(do_encode(
            input_bytes.unwrap(),
            palette_size,
            key,
            options.contains("z"),
        ))
    } else {
        Err(do_decode(
            input_bytes.unwrap_err(),
            key,
            options.contains("z"),
        ))
    };
    do_output(processed_data, args[3].as_str());
}
