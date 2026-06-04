//! HTML Form handling — data collection, validation, and submission encoding.
//!
//! Implements form data collection from `<form>` elements, input validation,
//! and URL-encoded / multipart form data encoding for submission.

use crate::{Document, ElementData, Node, NodeKind, NodeRef};
use smallvec::SmallVec;

/// A single form field (name-value pair) collected from a form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormField {
    pub name: String,
    pub value: String,
}

/// The encoding type for form submission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormEncoding {
    /// `application/x-www-form-urlencoded` (default)
    UrlEncoded,
    /// `multipart/form-data` (for file uploads)
    Multipart,
    /// `text/plain` (rarely used)
    TextPlain,
}

/// HTTP method for form submission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormMethod {
    Get,
    Post,
}

/// Collected form data ready for submission.
#[derive(Debug, Clone)]
pub struct FormData {
    pub fields: Vec<FormField>,
    pub action: String,
    pub method: FormMethod,
    pub encoding: FormEncoding,
}

/// Validation error for a single form field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    pub field_name: String,
    pub message: String,
    pub kind: ValidationKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationKind {
    Required,
    PatternMismatch,
    TypeMismatch,
    TooShort,
    TooLong,
    RangeUnderflow,
    RangeOverflow,
}

/// Validation result for an entire form.
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<ValidationError>,
}

impl FormData {
    /// Collect form data from a `<form>` element in the document.
    ///
    /// Walks the form's descendants to find successful controls:
    /// - `<input>` with a `name` attribute (excluding disabled, submit, reset, button types)
    /// - `<textarea>` with a `name` attribute
    /// - `<select>` with a `name` attribute (uses selected option's value)
    pub fn collect_from_form<'arena>(form_node: NodeRef<'arena>) -> Self {
        let element = form_node.as_element();

        let action = element
            .and_then(|el| el.attr("action"))
            .map(|s| s.to_string())
            .unwrap_or_default();

        let method = element
            .and_then(|el| el.attr("method"))
            .map(|s| {
                if s.eq_ignore_ascii_case("post") {
                    FormMethod::Post
                } else {
                    FormMethod::Get
                }
            })
            .unwrap_or(FormMethod::Get);

        let encoding = element
            .and_then(|el| el.attr("enctype"))
            .map(|s| {
                if s.eq_ignore_ascii_case("multipart/form-data") {
                    FormEncoding::Multipart
                } else if s.eq_ignore_ascii_case("text/plain") {
                    FormEncoding::TextPlain
                } else {
                    FormEncoding::UrlEncoded
                }
            })
            .unwrap_or(FormEncoding::UrlEncoded);

        let mut fields = Vec::new();
        collect_form_controls(form_node, &mut fields);

        Self {
            fields,
            action,
            method,
            encoding,
        }
    }

    /// Encode the form data as `application/x-www-form-urlencoded`.
    pub fn to_url_encoded(&self) -> String {
        self.fields
            .iter()
            .map(|f| {
                format!(
                    "{}={}",
                    url_encode(&f.name),
                    url_encode(&f.value)
                )
            })
            .collect::<Vec<_>>()
            .join("&")
    }

    /// Encode the form data as `text/plain`.
    pub fn to_text_plain(&self) -> String {
        self.fields
            .iter()
            .map(|f| format!("{}={}", f.name, f.value))
            .collect::<Vec<_>>()
            .join("\r\n")
    }

    /// Build the submission body bytes based on the encoding type.
    pub fn encode_body(&self) -> Vec<u8> {
        match self.encoding {
            FormEncoding::UrlEncoded => self.to_url_encoded().into_bytes(),
            FormEncoding::TextPlain => self.to_text_plain().into_bytes(),
            FormEncoding::Multipart => {
                // For multipart, use a simple boundary-based encoding
                let boundary = "----FortrustFormBoundary";
                let mut body = String::new();
                for field in &self.fields {
                    body.push_str(&format!("--{}\r\n", boundary));
                    body.push_str(&format!(
                        "Content-Disposition: form-data; name=\"{}\"\r\n\r\n",
                        field.name
                    ));
                    body.push_str(&field.value);
                    body.push_str("\r\n");
                }
                body.push_str(&format!("--{}--\r\n", boundary));
                body.into_bytes()
            }
        }
    }

    /// Build the Content-Type header value for the form encoding.
    pub fn content_type(&self) -> String {
        match self.encoding {
            FormEncoding::UrlEncoded => "application/x-www-form-urlencoded".to_owned(),
            FormEncoding::TextPlain => "text/plain".to_owned(),
            FormEncoding::Multipart => {
                "multipart/form-data; boundary=----FortrustFormBoundary".to_owned()
            }
        }
    }

    /// Build the full submission URL (for GET requests, appends query string).
    pub fn submission_url(&self, base_url: &str) -> String {
        let action = if self.action.is_empty() {
            base_url.to_owned()
        } else if self.action.starts_with("http://") || self.action.starts_with("https://") {
            self.action.clone()
        } else {
            // Resolve relative URL
            if let Ok(base) = url::Url::parse(base_url) {
                base.join(&self.action).map(|u| u.to_string()).unwrap_or(self.action.clone())
            } else {
                self.action.clone()
            }
        };

        if self.method == FormMethod::Get {
            let separator = if action.contains('?') { "&" } else { "?" };
            format!("{}{}{}", action, separator, self.to_url_encoded())
        } else {
            action
        }
    }
}

/// Validate all form controls within a form element.
pub fn validate_form<'arena>(form_node: NodeRef<'arena>) -> ValidationResult {
    let mut errors = Vec::new();
    validate_controls(form_node, &mut errors);
    ValidationResult {
        valid: errors.is_empty(),
        errors,
    }
}

/// Walk descendants and collect form control values.
fn collect_form_controls<'arena>(node: NodeRef<'arena>, fields: &mut Vec<FormField>) {
    let children = node.children();
    for child in children.iter().copied() {
        if let Some(el) = child.as_element() {
            let tag = el.local_name();

            // Skip disabled controls
            if el.attr("disabled").is_some() {
                continue;
            }

            match tag {
                "input" => {
                    if let Some(name) = el.attr("name").filter(|n| !n.is_empty()) {
                        let input_type = el.attr("type").unwrap_or_default().to_lowercase();
                        match input_type.as_str() {
                            "submit" | "reset" | "button" | "image" => {
                                // Not successful controls
                            }
                            "checkbox" => {
                                if el.attr("checked").is_some() {
                                    let value = el.attr("value").unwrap_or_else(|| "on".into());
                                    fields.push(FormField {
                                        name: name.to_string(),
                                        value: value.to_string(),
                                    });
                                }
                            }
                            "radio" => {
                                if el.attr("checked").is_some() {
                                    let value = el.attr("value").unwrap_or_else(|| "on".into());
                                    fields.push(FormField {
                                        name: name.to_string(),
                                        value: value.to_string(),
                                    });
                                }
                            }
                            "file" => {
                                // File inputs would need special handling
                                // For now, include the filename if available
                                if let Some(filename) = el.attr("data-filename") {
                                    fields.push(FormField {
                                        name: name.to_string(),
                                        value: filename.to_string(),
                                    });
                                }
                            }
                            _ => {
                                let value = el.attr("value").unwrap_or_default();
                                fields.push(FormField {
                                    name: name.to_string(),
                                    value: value.to_string(),
                                });
                            }
                        }
                    }
                }
                "textarea" => {
                    if let Some(name) = el.attr("name").filter(|n| !n.is_empty()) {
                        let value = child.text_content();
                        fields.push(FormField {
                            name: name.to_string(),
                            value,
                        });
                    }
                }
                "select" => {
                    if let Some(name) = el.attr("name").filter(|n| !n.is_empty()) {
                        // Find the selected option
                        let value = find_selected_option(child);
                        fields.push(FormField {
                            name: name.to_string(),
                            value,
                        });
                    }
                }
                _ => {}
            }
        }

        // Recurse into children (but not into nested forms)
        if child.as_element().map(|el| el.local_name()) != Some("form") {
            collect_form_controls(child, fields);
        }
    }
}

/// Find the value of the selected `<option>` in a `<select>` element.
fn find_selected_option<'arena>(select_node: NodeRef<'arena>) -> String {
    let children = select_node.children();
    for child in children.iter().copied() {
        if let Some(el) = child.as_element() {
            if el.local_name() == "option" {
                let is_selected = el.attr("selected").is_some();
                if is_selected {
                    return el
                        .attr("value")
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| child.text_content());
                }
            }
            // Check for <optgroup>
            if el.local_name() == "optgroup" {
                let result = find_selected_option(child);
                if !result.is_empty() {
                    return result;
                }
            }
        }
    }
    // If no option is explicitly selected, use the first option's value
    for child in children.iter().copied() {
        if let Some(el) = child.as_element() {
            if el.local_name() == "option" {
                return el
                    .attr("value")
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| child.text_content());
            }
        }
    }
    String::new()
}

/// Walk descendants and validate form controls.
fn validate_controls<'arena>(node: NodeRef<'arena>, errors: &mut Vec<ValidationError>) {
    let children = node.children();
    for child in children.iter().copied() {
        if let Some(el) = child.as_element() {
            let tag = el.local_name();
            let name = el.attr("name").unwrap_or_default().to_string();

            if tag == "input" || tag == "textarea" || tag == "select" {
                let value = if tag == "select" {
                    find_selected_option(child)
                } else if tag == "textarea" {
                    child.text_content()
                } else {
                    el.attr("value").unwrap_or_default().to_string()
                };

                // Check required
                if el.attr("required").is_some() && value.trim().is_empty() {
                    errors.push(ValidationError {
                        field_name: name.clone(),
                        message: "This field is required".to_owned(),
                        kind: ValidationKind::Required,
                    });
                }

                // Check pattern
                if let Some(pattern) = el.attr("pattern") {
                    if !value.is_empty() {
                        if let Ok(re) = simple_pattern_match(&pattern, &value) {
                            if !re {
                                errors.push(ValidationError {
                                    field_name: name.clone(),
                                    message: format!("Value does not match pattern: {}", pattern),
                                    kind: ValidationKind::PatternMismatch,
                                });
                            }
                        }
                    }
                }

                // Check minlength
                if let Some(min) = el.attr("minlength") {
                    if let Ok(min_len) = min.parse::<usize>() {
                        if !value.is_empty() && value.len() < min_len {
                            errors.push(ValidationError {
                                field_name: name.clone(),
                                message: format!("Minimum length is {}", min_len),
                                kind: ValidationKind::TooShort,
                            });
                        }
                    }
                }

                // Check maxlength
                if let Some(max) = el.attr("maxlength") {
                    if let Ok(max_len) = max.parse::<usize>() {
                        if value.len() > max_len {
                            errors.push(ValidationError {
                                field_name: name.clone(),
                                message: format!("Maximum length is {}", max_len),
                                kind: ValidationKind::TooLong,
                            });
                        }
                    }
                }

                // Type-specific validation for inputs
                if tag == "input" {
                    let input_type = el.attr("type").unwrap_or_default().to_lowercase();
                    match input_type.as_str() {
                        "email" => {
                            if !value.is_empty() && !is_valid_email(&value) {
                                errors.push(ValidationError {
                                    field_name: name.clone(),
                                    message: "Please enter a valid email address".to_owned(),
                                    kind: ValidationKind::TypeMismatch,
                                });
                            }
                        }
                        "url" => {
                            if !value.is_empty() && url::Url::parse(&value).is_err() {
                                errors.push(ValidationError {
                                    field_name: name.clone(),
                                    message: "Please enter a valid URL".to_owned(),
                                    kind: ValidationKind::TypeMismatch,
                                });
                            }
                        }
                        "number" | "range" => {
                            if !value.is_empty() {
                                if let Ok(num) = value.parse::<f64>() {
                                    if let Some(min) = el.attr("min") {
                                        if let Ok(min_val) = min.parse::<f64>() {
                                            if num < min_val {
                                                errors.push(ValidationError {
                                                    field_name: name.clone(),
                                                    message: format!("Value must be at least {}", min_val),
                                                    kind: ValidationKind::RangeUnderflow,
                                                });
                                            }
                                        }
                                    }
                                    if let Some(max) = el.attr("max") {
                                        if let Ok(max_val) = max.parse::<f64>() {
                                            if num > max_val {
                                                errors.push(ValidationError {
                                                    field_name: name.clone(),
                                                    message: format!("Value must be at most {}", max_val),
                                                    kind: ValidationKind::RangeOverflow,
                                                });
                                            }
                                        }
                                    }
                                } else {
                                    errors.push(ValidationError {
                                        field_name: name.clone(),
                                        message: "Please enter a number".to_owned(),
                                        kind: ValidationKind::TypeMismatch,
                                    });
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // Recurse into children (but not nested forms)
        if child.as_element().map(|el| el.local_name()) != Some("form") {
            validate_controls(child, errors);
        }
    }
}

/// Simple glob-style pattern matching for HTML `pattern` attribute.
/// Converts basic regex patterns (no groups, no backreferences).
fn simple_pattern_match(pattern: &str, value: &str) -> Result<bool, ()> {
    // Build a simple regex from the HTML pattern
    // HTML patterns are anchored: ^pattern$
    let regex_str = format!("^(?:{})$", pattern);
    // Use a simple check: try to parse as regex
    // For production, we'd use the `regex` crate, but to avoid adding a dependency,
    // we do a basic comparison
    if pattern == ".*" || pattern.is_empty() {
        return Ok(true);
    }
    // Very basic: check if pattern is a literal match or simple character class
    // This is intentionally simplified — a real implementation would use regex crate
    if !pattern.contains('[') && !pattern.contains('(') && !pattern.contains('|') && !pattern.contains('*') && !pattern.contains('+') && !pattern.contains('?') && !pattern.contains('\\') && !pattern.contains('{') {
        // Literal pattern — must match exactly
        return Ok(value == pattern);
    }
    // For complex patterns, accept the value (fail-open for now)
    Ok(true)
}

/// Basic email validation (checks for @ and a dot after @).
fn is_valid_email(email: &str) -> bool {
    let at_pos = email.find('@');
    let Some(at_pos) = at_pos else { return false };
    if at_pos == 0 || at_pos == email.len() - 1 {
        return false;
    }
    let domain = &email[at_pos + 1..];
    domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
}

/// URL-encode a string for `application/x-www-form-urlencoded`.
pub fn url_encode(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char);
            }
            b' ' => result.push('+'),
            _ => {
                result.push('%');
                result.push_str(&format!("{:02X}", byte));
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DomArena, parse_html};

    #[test]
    fn collect_simple_form_data() {
        let arena = DomArena::new();
        let doc = parse_html(
            &arena,
            r#"<form action="/submit" method="post">
                <input name="username" value="alice">
                <input name="password" type="password" value="secret123">
                <input type="submit" value="Login">
            </form>"#,
        )
        .unwrap();

        let form = doc.first_element_by_tag("form").unwrap();
        let data = FormData::collect_from_form(form);

        assert_eq!(data.action, "/submit");
        assert_eq!(data.method, FormMethod::Post);
        assert_eq!(data.encoding, FormEncoding::UrlEncoded);
        assert_eq!(data.fields.len(), 2);
        assert_eq!(data.fields[0].name, "username");
        assert_eq!(data.fields[0].value, "alice");
        assert_eq!(data.fields[1].name, "password");
        assert_eq!(data.fields[1].value, "secret123");
    }

    #[test]
    fn collect_checkbox_and_radio() {
        let arena = DomArena::new();
        let doc = parse_html(
            &arena,
            r#"<form>
                <input type="checkbox" name="agree" value="yes" checked>
                <input type="checkbox" name="newsletter" value="yes">
                <input type="radio" name="color" value="red">
                <input type="radio" name="color" value="blue" checked>
            </form>"#,
        )
        .unwrap();

        let form = doc.first_element_by_tag("form").unwrap();
        let data = FormData::collect_from_form(form);

        assert_eq!(data.fields.len(), 2);
        assert_eq!(data.fields[0], FormField { name: "agree".into(), value: "yes".into() });
        assert_eq!(data.fields[1], FormField { name: "color".into(), value: "blue".into() });
    }

    #[test]
    fn collect_textarea_and_select() {
        let arena = DomArena::new();
        let doc = parse_html(
            &arena,
            r#"<form>
                <textarea name="bio">Hello world</textarea>
                <select name="country">
                    <option value="us">United States</option>
                    <option value="uk" selected>United Kingdom</option>
                </select>
            </form>"#,
        )
        .unwrap();

        let form = doc.first_element_by_tag("form").unwrap();
        let data = FormData::collect_from_form(form);

        assert_eq!(data.fields.len(), 2);
        assert_eq!(data.fields[0].name, "bio");
        assert!(data.fields[0].value.contains("Hello world"));
        assert_eq!(data.fields[1].name, "country");
        assert_eq!(data.fields[1].value, "uk");
    }

    #[test]
    fn url_encoding_special_characters() {
        assert_eq!(url_encode("hello world"), "hello+world");
        assert_eq!(url_encode("a=b&c=d"), "a%3Db%26c%3Dd");
        assert_eq!(url_encode("test@example.com"), "test%40example.com");
        assert_eq!(url_encode("plain"), "plain");
    }

    #[test]
    fn form_url_encoded_output() {
        let data = FormData {
            fields: vec![
                FormField { name: "q".into(), value: "rust browser".into() },
                FormField { name: "page".into(), value: "1".into() },
            ],
            action: String::new(),
            method: FormMethod::Get,
            encoding: FormEncoding::UrlEncoded,
        };
        assert_eq!(data.to_url_encoded(), "q=rust+browser&page=1");
    }

    #[test]
    fn get_submission_url_appends_query() {
        let data = FormData {
            fields: vec![FormField { name: "q".into(), value: "test".into() }],
            action: "https://example.com/search".into(),
            method: FormMethod::Get,
            encoding: FormEncoding::UrlEncoded,
        };
        let url = data.submission_url("https://example.com/");
        assert_eq!(url, "https://example.com/search?q=test");
    }

    #[test]
    fn post_submission_url_unchanged() {
        let data = FormData {
            fields: vec![FormField { name: "user".into(), value: "alice".into() }],
            action: "https://example.com/login".into(),
            method: FormMethod::Post,
            encoding: FormEncoding::UrlEncoded,
        };
        let url = data.submission_url("https://example.com/");
        assert_eq!(url, "https://example.com/login");
    }

    #[test]
    fn validate_required_field_missing() {
        let arena = DomArena::new();
        let doc = parse_html(
            &arena,
            r#"<form><input name="email" required value=""></form>"#,
        )
        .unwrap();

        let form = doc.first_element_by_tag("form").unwrap();
        let result = validate_form(form);

        assert!(!result.valid);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].kind, ValidationKind::Required);
        assert_eq!(result.errors[0].field_name, "email");
    }

    #[test]
    fn validate_required_field_present() {
        let arena = DomArena::new();
        let doc = parse_html(
            &arena,
            r#"<form><input name="email" required value="test@example.com"></form>"#,
        )
        .unwrap();

        let form = doc.first_element_by_tag("form").unwrap();
        let result = validate_form(form);

        assert!(result.valid);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn validate_email_type() {
        let arena = DomArena::new();
        let doc = parse_html(
            &arena,
            r#"<form><input type="email" name="email" value="not-an-email"></form>"#,
        )
        .unwrap();

        let form = doc.first_element_by_tag("form").unwrap();
        let result = validate_form(form);

        assert!(!result.valid);
        assert_eq!(result.errors[0].kind, ValidationKind::TypeMismatch);
    }

    #[test]
    fn validate_number_range() {
        let arena = DomArena::new();
        let doc = parse_html(
            &arena,
            r#"<form><input type="number" name="age" min="0" max="150" value="200"></form>"#,
        )
        .unwrap();

        let form = doc.first_element_by_tag("form").unwrap();
        let result = validate_form(form);

        assert!(!result.valid);
        assert_eq!(result.errors[0].kind, ValidationKind::RangeOverflow);
    }

    #[test]
    fn validate_maxlength() {
        let arena = DomArena::new();
        let doc = parse_html(
            &arena,
            r#"<form><input name="code" maxlength="4" value="12345"></form>"#,
        )
        .unwrap();

        let form = doc.first_element_by_tag("form").unwrap();
        let result = validate_form(form);

        assert!(!result.valid);
        assert_eq!(result.errors[0].kind, ValidationKind::TooLong);
    }

    #[test]
    fn disabled_inputs_are_excluded() {
        let arena = DomArena::new();
        let doc = parse_html(
            &arena,
            r#"<form>
                <input name="active" value="yes">
                <input name="disabled_field" value="no" disabled>
            </form>"#,
        )
        .unwrap();

        let form = doc.first_element_by_tag("form").unwrap();
        let data = FormData::collect_from_form(form);

        assert_eq!(data.fields.len(), 1);
        assert_eq!(data.fields[0].name, "active");
    }

    #[test]
    fn email_validation() {
        assert!(is_valid_email("user@example.com"));
        assert!(is_valid_email("test@sub.domain.org"));
        assert!(!is_valid_email("no-at-sign"));
        assert!(!is_valid_email("@no-local.com"));
        assert!(!is_valid_email("user@"));
        assert!(!is_valid_email("user@.com"));
    }
}
