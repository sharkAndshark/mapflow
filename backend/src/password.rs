use bcrypt::{hash, verify, DEFAULT_COST};

const PASSWORD_MIN_LENGTH: usize = 8;
const PASSWORD_MAX_LENGTH: usize = 128;

/// Errors related to password operations
#[derive(Debug)]
pub enum PasswordError {
    TooShort,
    TooLong,
    MissingUppercase,
    MissingLowercase,
    MissingDigit,
    MissingSpecial,
    HashFailed(String),
    VerifyFailed(String),
}

impl std::fmt::Display for PasswordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PasswordError::TooShort => write!(
                f,
                "Password must be at least {} characters",
                PASSWORD_MIN_LENGTH
            ),
            PasswordError::TooLong => write!(
                f,
                "Password must be at most {} characters",
                PASSWORD_MAX_LENGTH
            ),
            PasswordError::MissingUppercase => {
                write!(f, "Password must contain at least one uppercase letter")
            }
            PasswordError::MissingLowercase => {
                write!(f, "Password must contain at least one lowercase letter")
            }
            PasswordError::MissingDigit => write!(f, "Password must contain at least one digit"),
            PasswordError::MissingSpecial => {
                write!(f, "Password must contain at least one special character")
            }
            PasswordError::HashFailed(msg) => write!(f, "Failed to hash password: {}", msg),
            PasswordError::VerifyFailed(msg) => write!(f, "Failed to verify password: {}", msg),
        }
    }
}

impl std::error::Error for PasswordError {}

/// Validate password complexity requirements
///
/// Requirements:
/// - At least 8 characters
/// - At least one uppercase letter (A-Z)
/// - At least one lowercase letter (a-z)
/// - At least one digit (0-9)
/// - At least one special character (!@#$%^&* etc.)
pub fn validate_password_complexity(password: &str) -> Result<(), PasswordError> {
    if password.len() < PASSWORD_MIN_LENGTH {
        return Err(PasswordError::TooShort);
    }
    if password.len() > PASSWORD_MAX_LENGTH {
        return Err(PasswordError::TooLong);
    }

    let has_upper = password.chars().any(|c| c.is_ascii_uppercase());
    let has_lower = password.chars().any(|c| c.is_ascii_lowercase());
    let has_digit = password.chars().any(|c| c.is_ascii_digit());
    let has_special = password.chars().any(|c| {
        matches!(
            c,
            '!' | '@'
                | '#'
                | '$'
                | '%'
                | '^'
                | '&'
                | '*'
                | '_'
                | '-'
                | '+'
                | '='
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | '|'
                | '\\'
                | ':'
                | ';'
                | '"'
                | '\''
                | '<'
                | '>'
                | ','
                | '.'
                | '?'
                | '/'
                | '~'
                | '`'
        )
    });

    if !has_upper {
        return Err(PasswordError::MissingUppercase);
    }
    if !has_lower {
        return Err(PasswordError::MissingLowercase);
    }
    if !has_digit {
        return Err(PasswordError::MissingDigit);
    }
    if !has_special {
        return Err(PasswordError::MissingSpecial);
    }

    Ok(())
}

/// Hash a password using bcrypt
///
/// Uses bcrypt with default cost factor (10)
pub fn hash_password(password: &str) -> Result<String, PasswordError> {
    hash(password, DEFAULT_COST).map_err(|e| PasswordError::HashFailed(e.to_string()))
}

/// Verify a password against a hash
///
/// Uses constant-time comparison to prevent timing attacks
pub fn verify_password(password: &str, hash: &str) -> Result<bool, PasswordError> {
    verify(password, hash).map_err(|e| PasswordError::VerifyFailed(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_password_complexity_valid() {
        let password = "Test123!@#";
        assert!(validate_password_complexity(password).is_ok());
    }

    #[test]
    fn test_validate_password_too_short() {
        let password = "Test1!";
        assert!(matches!(
            validate_password_complexity(password),
            Err(PasswordError::TooShort)
        ));
    }

    #[test]
    fn test_validate_password_missing_uppercase() {
        let password = "test123!@#";
        assert!(matches!(
            validate_password_complexity(password),
            Err(PasswordError::MissingUppercase)
        ));
    }

    #[test]
    fn test_validate_password_missing_lowercase() {
        let password = "TEST123!@#";
        assert!(matches!(
            validate_password_complexity(password),
            Err(PasswordError::MissingLowercase)
        ));
    }

    #[test]
    fn test_validate_password_missing_digit() {
        let password = "TestTest!@#";
        assert!(matches!(
            validate_password_complexity(password),
            Err(PasswordError::MissingDigit)
        ));
    }

    #[test]
    fn test_validate_password_missing_special() {
        let password = "Test123456";
        assert!(matches!(
            validate_password_complexity(password),
            Err(PasswordError::MissingSpecial)
        ));
    }

    #[test]
    fn test_hash_and_verify_password() {
        let password = "Test123!@#";
        let hash = hash_password(password).unwrap();
        assert!(verify_password(password, &hash).unwrap());
        assert!(!verify_password("wrongpassword", &hash).unwrap());
    }

    #[test]
    fn test_hash_twice_different_results() {
        let password = "Test123!@#";
        let hash1 = hash_password(password).unwrap();
        let hash2 = hash_password(password).unwrap();
        assert_ne!(
            hash1, hash2,
            "Hashing same password twice should produce different results due to salt"
        );
    }

    #[test]
    fn test_hash_starts_with_bcrypt_prefix() {
        let password = "Test123!@#";
        let hash = hash_password(password).unwrap();
        assert!(
            hash.starts_with("$2b$"),
            "Bcrypt hash should start with $2b$ prefix"
        );
    }
}
