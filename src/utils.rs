use std::env;

/// Reads an environment variable as a boolean.
///
/// If the environment variable exists and matches one of the following case-insensitive values:
///     "true"
///     "yes"
///     "y"
///     "1"
/// Then this function will evaluate to true. False otherwise.
pub fn bool_env_var(name: &str) -> bool {
    match env::var(name) {
        Ok(val) => matches!(val.trim().to_lowercase().as_str(), "1" | "y" | "yes" | "true"),
        Err(_) => false,
    }
}

/// Reads an environment variable as an unsigned integer.
///
/// If the environment variable exists and can be parsed as an unsigned integer, then this
/// function will return the value as Some(usize). Otherwise, it will return None.
pub fn unsigned_env_var(name: &str) -> Option<usize> {
    match env::var(name) {
        Ok(val) => val.parse().ok(),
        Err(_) => None,
    }
}
