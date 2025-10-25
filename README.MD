Tool for images coding and chipering

**!!!Warning!!!**

- **during encoding input image is rewritten with a new pallete of a specified size (2 as min, 257 as max)**

**Tool features:**

- Encoding as palette-based image with the Floyd-Steinberg dithering. Palette is generated with median-cut algorithm
- AES128 encryption-decryption (length-preserving) for pixels data available (header data stays unchanged)
- Zstandard compression for pixels data available

**Build:**

    cargo build --release

**Usage:**

    exe [options] [input_file_path] [output_file_path] [palette_size(2..257 inclusively)] [base64url_key]

    options:
        - i - info mode: input - existing [input_file_path], output - stderr (correctly shown only for not encrypted and not compressed images)
        - e - encode mode: additional input - [palette_size], output - saved [output_file_path] or stderr
        - d - decode mode: output - saved [output_file_path] or stderr
        - c - encryption-decryption flag: additional [base64url_key] arg at last position
        - z - compression-decompression flag
        - g - 16bytes base64url stdout key gen

**Examples:**

- image for encoding (143KB):
![image.png](./image.png)

- ./target/release/rust_image-codec ecz image.png encoded.bin 256 dozR7XYsRBdDmHhHtRmCJg:
    *saved ./encoded.bin file (62KB)

- ./target/release/rust_image-codec dcz encoded.bin decoded.png dozR7XYsRBdDmHhHtRmCJg:
    *saved ./decoded.png image

- decoded image (145KB):
![decoded.png](./decoded.png)

**Encoded header format:**

- first three bytes - image dimensions (both are 2-based)
- fourth byte - palette size (2-based)
- next 3 * {palette_size} bytes - rgb8 colors
- rest of bytes - pixels data as byte-sized palette indices
