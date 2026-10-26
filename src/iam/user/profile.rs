pub fn format_name(given_name: &str, family_name: &str) -> String {
    if given_name.is_empty() && family_name.is_empty() {
        return "Anonymous".to_string();
    }
    if given_name.is_empty() {
        return family_name.to_string();
    }

    if family_name.is_empty() {
        return given_name.to_string();
    }

    format!("{} {}", given_name, family_name).trim().to_string()
}
