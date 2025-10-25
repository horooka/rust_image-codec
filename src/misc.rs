use image::{DynamicImage, ImageBuffer, ImageFormat, Rgb, imageops::ColorMap};
use itertools::Itertools;
use rand::{Rng, rng};
use std::{io::Write, process::exit};

const PROGRESS_BAR_WIDTH: usize = 50;

pub struct ProgressBar {
    pub last_step: usize,
    current_step: usize,
}

impl ProgressBar {
    pub fn new(last_step: usize) -> Self {
        Self {
            last_step,
            current_step: 0,
        }
    }

    pub fn step(&mut self) {
        self.current_step = (self.current_step + 1).min(self.last_step);
        let percent = self.current_step as f32 / self.last_step as f32 * 100.0;
        let done_width = (percent / 100.0 * PROGRESS_BAR_WIDTH as f32) as usize;

        print!("\r{}", " ".repeat(PROGRESS_BAR_WIDTH));
        print!(
            "\rProcessing... [{}{}] ({}%)",
            "|".repeat(done_width),
            " ".repeat(PROGRESS_BAR_WIDTH - done_width),
            percent as usize
        );
        use std::io::{Write, stdout};
        stdout().flush().unwrap();
    }

    pub fn step_percent(&mut self, percent: f32) {
        for _ in 0..(percent * PROGRESS_BAR_WIDTH as f32) as usize {
            self.step();
        }
    }
}

pub struct Palette {
    pub colors: Vec<Rgb<u8>>,
}

impl ColorMap for Palette {
    type Color = Rgb<u8>;

    fn index_of(&self, color: &Self::Color) -> usize {
        self.colors
            .iter()
            .enumerate()
            .min_by_key(|&(_, rgb)| {
                let r = rgb[0] as i32 - color[0] as i32;
                let g = rgb[1] as i32 - color[1] as i32;
                let b = rgb[2] as i32 - color[2] as i32;
                r * r + g * g + b * b
            })
            .map(|(idx, _)| idx)
            .unwrap_or(0)
    }

    fn map_color(&self, color: &mut Self::Color) {
        let idx = self.index_of(color);
        let rgb = self.colors.to_owned()[idx];
        *color = Rgb([rgb[0], rgb[1], rgb[2]]);
    }
}

struct Bucket {
    pixels: Vec<Rgb<u8>>,
}

impl Bucket {
    fn new(pixels: Vec<Rgb<u8>>) -> Self {
        Self { pixels }
    }

    fn largest_range_channel(&self) -> usize {
        let (min_r, max_r) = self
            .pixels
            .iter()
            .map(|p| p[0])
            .minmax()
            .into_option()
            .unwrap();
        let (min_g, max_g) = self
            .pixels
            .iter()
            .map(|p| p[1])
            .minmax()
            .into_option()
            .unwrap();
        let (min_b, max_b) = self
            .pixels
            .iter()
            .map(|p| p[2])
            .minmax()
            .into_option()
            .unwrap();

        let range_r = max_r - min_r;
        let range_g = max_g - min_g;
        let range_b = max_b - min_b;

        if range_r >= range_g && range_r >= range_b {
            0
        } else if range_g >= range_r && range_g >= range_b {
            1
        } else {
            2
        }
    }

    fn split(self) -> (Self, Self) {
        let ch = self.largest_range_channel();
        let mut pixels = self.pixels;
        pixels.sort_unstable_by_key(|p| p[ch]);

        let mid = pixels.len() / 2;
        let lower = pixels[..mid].to_vec();
        let upper = pixels[mid..].to_vec();

        (Self::new(lower), Self::new(upper))
    }

    fn average_color(&self) -> Rgb<u8> {
        let len = self.pixels.len() as u32;
        let (r_sum, g_sum, b_sum) =
            self.pixels
                .iter()
                .fold((0u32, 0u32, 0u32), |(r_acc, g_acc, b_acc), p| {
                    (
                        r_acc + p[0] as u32,
                        g_acc + p[1] as u32,
                        b_acc + p[2] as u32,
                    )
                });
        Rgb([
            (r_sum / len) as u8,
            (g_sum / len) as u8,
            (b_sum / len) as u8,
        ])
    }

    fn variance(&self) -> u32 {
        let len = self.pixels.len() as u32;
        if len == 0 {
            return 0;
        }

        let avg = self.average_color();
        self.pixels
            .iter()
            .map(|p| {
                let dr = p[0] as i32 - avg[0] as i32;
                let dg = p[1] as i32 - avg[1] as i32;
                let db = p[2] as i32 - avg[2] as i32;
                (dr * dr + dg * dg + db * db) as u32
            })
            .sum::<u32>()
            / len
    }
}

fn bytes_to_base64url(bytes: &[u8]) -> String {
    base64_url::encode(bytes)
}

pub fn base64url_to_bytes(code: &str) -> Option<Vec<u8>> {
    base64_url::decode(code).ok()
}

pub fn pack_dimensions(width: u16, height: u16) -> [u8; 3] {
    let combined: u32 = ((width as u32) << 12) | (height as u32);

    [
        ((combined >> 16) & 0xFF) as u8,
        ((combined >> 8) & 0xFF) as u8,
        (combined & 0xFF) as u8,
    ]
}

pub fn unpack_dimensions(bytes: &[u8]) -> (u32, u32) {
    let combined: u32 = ((bytes[0] as u32) << 16) | ((bytes[1] as u32) << 8) | (bytes[2] as u32);

    let width = (combined >> 12) as u16 & 0xFFF;
    let height = (combined & 0xFFF) as u16;

    (width as u32, height as u32)
}

pub fn write_file(bytes: &[u8], output_file_path: &str) {
    match std::fs::File::create(output_file_path) {
        Ok(mut file) => {
            if let Some(err) = file.write_all(bytes).err() {
                eprintln!("Error: {}", err);
                exit(1);
            }
        }
        Err(err) => {
            eprintln!("Error: {}", err);
            exit(1);
        }
    }
}

pub fn gen_palette(pixels: &[Rgb<u8>], n: usize) -> Vec<Rgb<u8>> {
    let mut buckets = vec![Bucket::new(pixels.to_vec())];
    while buckets.len() < n {
        if let Some((idx, _)) = buckets
            .iter()
            .enumerate()
            .max_by_key(|&(_, b)| b.variance())
        {
            let bucket = buckets.swap_remove(idx);
            if bucket.pixels.len() <= 1 {
                buckets.push(bucket);
                break;
            }

            let (b1, b2) = bucket.split();
            buckets.push(b1);
            buckets.push(b2);
        } else {
            break;
        }
    }

    buckets.iter().map(|b| b.average_color()).collect()
}

pub fn decode_palette(bytes: &[u8]) -> Vec<Rgb<u8>> {
    let mut palette: Vec<Rgb<u8>> = Vec::new();
    for i in 0..bytes.len() / 3 {
        let rgb = [bytes[i * 3], bytes[i * 3 + 1], bytes[i * 3 + 2]];
        palette.push(Rgb(rgb));
    }
    palette
}

pub fn open_img(path: &str) -> Result<ImageBuffer<Rgb<u8>, Vec<u8>>, image::ImageError> {
    let img: ImageBuffer<Rgb<u8>, Vec<u8>> = image::ImageReader::open(path)?.decode()?.into_rgb8();
    Ok(img)
}

pub fn save_img(
    img: ImageBuffer<Rgb<u8>, Vec<u8>>,
    output_file_path: &str,
) -> Result<(), image::ImageError> {
    match DynamicImage::ImageRgb8(img).save_with_format(output_file_path, ImageFormat::Png) {
        Ok(_) => Ok(()),
        Err(err) => Err(err),
    }
}

pub fn get_info(file_path: &str) -> String {
    let bytes = std::fs::read(file_path).unwrap();
    let (width, height) = unpack_dimensions(&bytes[0..3]);
    format!(
        "width: {}, height: {}, palette_size: {}",
        width + 2,
        height + 2,
        bytes[3] as usize + 2,
    )
}

pub fn gen_key() -> String {
    let mut rng = rng();
    bytes_to_base64url(
        (0..16)
            .map(|_| rng.random())
            .collect::<Vec<u8>>()
            .as_slice(),
    )
}
