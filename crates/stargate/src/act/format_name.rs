pub fn format_name(first_name: &str, last_name: &str) -> String {
    if first_name.is_empty() && last_name.is_empty() {
        return "Anonymous".to_string();
    }
    if first_name.is_empty() {
        return last_name.to_string();
    }

    if last_name.is_empty() {
        return first_name.to_string();
    }

    format!("{} {}", first_name, last_name).trim().to_string()
}
