use apiks::generate_api_key;

fn main() {
    let api_key = generate_api_key();
    println!("Generated API Key: {}", api_key);
    let hashed_key = apiks::hash_api_key(&api_key);
    println!("Hashed API Key: {}", hashed_key);
}
