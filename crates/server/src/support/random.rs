//! Random helpers, mirroring `utils/tools.go` (`RandomString`).

use rand::Rng;

const LETTER_BYTES: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

pub fn random_string(n: usize) -> String {
    let mut rng = rand::thread_rng();
    (0..n)
        .map(|_| {
            let idx = rng.gen_range(0..LETTER_BYTES.len());
            LETTER_BYTES[idx] as char
        })
        .collect()
}
