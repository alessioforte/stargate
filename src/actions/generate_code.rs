use rand::Rng;

/// Generates a random numeric code of the specified length.
///
/// # Arguments
///
/// * `length` - The length of the code to be generated.
///
/// # Returns
///
/// A `String` containing the generated numeric code.
///
/// # Examples
///
/// ```
/// let code = generate_code(6);
/// println!("Generated code: {}", code);
/// // Output might be: "483920"
/// ```
///
/// # Panics
///
/// This function will panic if `length` is greater than `usize::MAX`.
///
/// # Notes
///
/// The generated code consists only of numeric digits (0-9).
pub fn generate_code(length: usize) -> String {
    let digits = "0123456789";
    let mut rng = rand::thread_rng();
    let code: String = (0..length)
        .map(|_| {
            let idx = rng.gen_range(0..digits.len());
            digits.chars().nth(idx).unwrap()
        })
        .collect();
    code
}
