//! Variables: named values a forward can reference instead of repeating a
//! username or password in every forward (spec 03 §24, 04 §14).
//!
//! - A **plain** variable holds a non-secret value (kept in the config file) and
//!   may appear in a forward's SSH username as `{{NAME}}`.
//! - A **secret** variable holds a password, kept in the credential store
//!   (keychain / fallback file) under the account [`secret_account`] — never in
//!   the config file, a backup, a log line or any IPC/socket/HTTP response. A
//!   forward uses it as its password via `passwordVariable`.
//!
//! Values are resolved when a tunnel dials, so a change applies on the next
//! (re)connect. This module is pure: parsing, validation and rendering only.

use std::collections::BTreeSet;

use crate::error::AppError;

/// Longest variable name.
pub const NAME_MAX: usize = 64;
/// Longest plain value (it ends up in an SSH username).
pub const VALUE_MAX: usize = 256;
/// Longest secret value.
pub const SECRET_MAX: usize = 4096;

/// Credential-store account of a secret variable. Forward passwords are keyed
/// by the forward id, a uuid (never containing `:`): every create mints one and
/// backup import replaces any other id, so the two cannot collide.
pub fn secret_account(name: &str) -> String {
    format!("var:{name}")
}

/// A valid name: a letter or `_`, then letters, digits or `_` (like an
/// environment variable), at most [`NAME_MAX`] characters.
pub fn validate_name(name: &str) -> Result<(), AppError> {
    let mut chars = name.chars();
    let valid_start = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    let valid_rest = chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !valid_start || !valid_rest || name.len() > NAME_MAX {
        return Err(AppError::InvalidInput(format!(
            "invalid variable name '{name}': use letters, digits and _ (not starting \
             with a digit), at most {NAME_MAX} characters"
        )));
    }
    Ok(())
}

/// A plain value: non-empty, at most [`VALUE_MAX`] characters, no control
/// characters (it becomes part of an SSH username).
pub fn validate_plain_value(name: &str, value: &str) -> Result<(), AppError> {
    if value.is_empty() {
        return Err(AppError::InvalidInput(format!(
            "variable {name} needs a value"
        )));
    }
    if value.chars().count() > VALUE_MAX {
        return Err(AppError::InvalidInput(format!(
            "variable {name} is longer than {VALUE_MAX} characters"
        )));
    }
    if value
        .chars()
        .any(|c| c.is_control() || is_invisible_format_char(c))
    {
        return Err(AppError::InvalidInput(format!(
            "variable {name} contains a control or invisible formatting character"
        )));
    }
    if value.starts_with('-') {
        // It becomes the start of `user@host`, which `ssh` would read as an
        // option (e.g. `-oProxyCommand=…`) in a copied command.
        return Err(AppError::InvalidInput(format!(
            "variable {name} cannot start with '-'"
        )));
    }
    if value.contains("{{") {
        return Err(AppError::InvalidInput(format!(
            "variable {name} cannot contain '{{{{' (variables do not nest)"
        )));
    }
    Ok(())
}

/// Zero-width and bidirectional-override characters: invisible, and able to
/// make a displayed username read differently from what is sent.
fn is_invisible_format_char(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{061C}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{FEFF}'
            | '\u{034F}'
            | '\u{115F}'
            | '\u{1160}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{3164}'
            | '\u{FFA0}'
            | '\u{E0000}'..='\u{E007F}'
    )
}

/// A secret value: non-empty and at most [`SECRET_MAX`] bytes. The message
/// never includes the value.
pub fn validate_secret_value(name: &str, value: &str) -> Result<(), AppError> {
    if value.is_empty() {
        return Err(AppError::InvalidInput(format!(
            "variable {name}: the secret value is empty"
        )));
    }
    if value.len() > SECRET_MAX {
        return Err(AppError::InvalidInput(format!(
            "variable {name}: the secret value is longer than {SECRET_MAX} bytes"
        )));
    }
    Ok(())
}

/// One piece of a template.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment<'a> {
    Text(&'a str),
    Var(&'a str),
}

/// Split `template` into literal text and `{{NAME}}` references (spaces inside
/// the braces are allowed: `{{ NAME }}`). An unclosed `{{` or an invalid name
/// is an error naming the problem.
fn parse(template: &str) -> Result<Vec<Segment<'_>>, AppError> {
    let mut segments = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        if start > 0 {
            segments.push(Segment::Text(&rest[..start]));
        }
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            return Err(AppError::InvalidInput(format!(
                "'{template}' has an unclosed '{{{{' — write a variable as {{{{NAME}}}}"
            )));
        };
        let name = after[..end].trim();
        validate_name(name)?;
        segments.push(Segment::Var(name));
        rest = &after[end + 2..];
    }
    if !rest.is_empty() {
        segments.push(Segment::Text(rest));
    }
    Ok(segments)
}

/// Every variable name `template` references, deduplicated and sorted.
pub fn referenced_names(template: &str) -> Result<BTreeSet<String>, AppError> {
    Ok(parse(template)?
        .into_iter()
        .filter_map(|s| match s {
            Segment::Var(name) => Some(name.to_string()),
            Segment::Text(_) => None,
        })
        .collect())
}

/// Whether `template` references `name` (false for an unparsable template).
pub fn references(template: &str, name: &str) -> bool {
    referenced_names(template).is_ok_and(|names| names.contains(name))
}

/// Substitute every `{{NAME}}` using `lookup`. A name `lookup` cannot resolve
/// is an error, so a tunnel never dials with a half-rendered username.
pub fn render<F>(template: &str, mut lookup: F) -> Result<String, AppError>
where
    F: FnMut(&str) -> Option<String>,
{
    let mut out = String::with_capacity(template.len());
    for segment in parse(template)? {
        match segment {
            Segment::Text(text) => out.push_str(text),
            Segment::Var(name) => match lookup(name) {
                Some(value) => out.push_str(&value),
                None => {
                    return Err(AppError::InvalidInput(format!(
                        "variable {{{{{name}}}}} is not defined (Settings → Variables, or `var set {name} <value>`)"
                    )))
                }
            },
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(name: &str) -> Option<String> {
        match name {
            "DB_USER" => Some("kalfian".into()),
            "ENV" => Some("prod".into()),
            _ => None,
        }
    }

    #[test]
    fn names_follow_env_var_rules() {
        for ok in ["DB_USER", "_x", "a1", "A"] {
            validate_name(ok).expect(ok);
        }
        for bad in [
            "",
            "1A",
            "DB-USER",
            "DB USER",
            "é",
            &"A".repeat(NAME_MAX + 1),
        ] {
            assert!(validate_name(bad).is_err(), "{bad:?} must be rejected");
        }
    }

    #[test]
    fn render_substitutes_whole_and_embedded_references() {
        assert_eq!(render("{{DB_USER}}", lookup).expect("whole"), "kalfian");
        assert_eq!(
            render("{{ DB_USER }}-{{ENV}}", lookup).expect("mixed"),
            "kalfian-prod"
        );
        assert_eq!(render("deploy", lookup).expect("literal"), "deploy");
        assert_eq!(render("a}}b", lookup).expect("stray close"), "a}}b");
    }

    #[test]
    fn render_refuses_unknown_and_malformed_references() {
        let err = render("{{NOPE}}", lookup).expect_err("unknown");
        assert!(err.to_string().contains("NOPE"));
        assert!(render("{{DB_USER", lookup).is_err());
        assert!(render("{{bad name}}", lookup).is_err());
        assert!(render("{{}}", lookup).is_err());
    }

    #[test]
    fn referenced_names_are_deduplicated() {
        let names = referenced_names("{{B}}{{A}}{{ B }}x").expect("parse");
        assert_eq!(names.into_iter().collect::<Vec<_>>(), vec!["A", "B"]);
        assert!(references("{{A}}@x", "A"));
        assert!(!references("{{A}}", "B"));
        assert!(!references("{{A", "A"));
    }

    #[test]
    fn values_are_validated_without_echoing_secrets() {
        validate_plain_value("U", "kalfian").expect("plain");
        assert!(validate_plain_value("U", "").is_err());
        assert!(validate_plain_value("U", "a\nb").is_err());
        assert!(validate_plain_value("U", "{{X}}").is_err());
        assert!(validate_plain_value("U", "admin\u{202E}nimda").is_err());
        assert!(validate_plain_value("U", "a\u{200B}b").is_err());
        assert!(validate_plain_value("U", "-oProxyCommand=sh").is_err());
        validate_plain_value("U", "ops-team").expect("a dash inside is fine");
        validate_secret_value("P", "hunter2").expect("secret");
        let err = validate_secret_value("P", &"x".repeat(SECRET_MAX + 1)).expect_err("long");
        assert!(!err.to_string().contains("xxxx"));
        assert!(validate_secret_value("P", "").is_err());
    }

    #[test]
    fn secret_accounts_cannot_collide_with_forward_ids() {
        let account = secret_account("DB_PASS");
        assert_eq!(account, "var:DB_PASS");
        assert!(uuid::Uuid::parse_str(&account).is_err());
    }
}
