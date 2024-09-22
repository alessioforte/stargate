use rand::Rng;

// generata 6 digit code
pub fn generate_code() -> String {
    let digits = "0123456789";
    let mut rng = rand::thread_rng();
    let code: String = (0..6)
        .map(|_| {
            let idx = rng.gen_range(0..digits.len());
            digits.chars().nth(idx).unwrap()
        })
        .collect();
    code
}
