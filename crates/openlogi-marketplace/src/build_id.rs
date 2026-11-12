//! Finding the marketplace site's current build identifier.

/// Extract the Next.js build identifier from a marketplace HTML page.
///
/// The site serves its catalog as per-build JSON documents, so every data URL
/// embeds this identifier, and it changes whenever the site is redeployed.
#[must_use]
pub fn extract_build_id(html: &str) -> Option<&str> {
    const KEY: &str = "\"buildId\"";
    let mut rest = html;
    while let Some(found) = rest.find(KEY) {
        rest = &rest[found + KEY.len()..];
        let Some(value) = rest
            .trim_start()
            .strip_prefix(':')
            .and_then(|after| after.trim_start().strip_prefix('"'))
        else {
            continue;
        };
        let id = &value[..value.find('"')?];
        // The identifier is spliced into a URL path: accept only what a build
        // id is made of, so page content cannot steer the request elsewhere.
        let plain = !id.is_empty()
            && id
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'));
        return plain.then_some(id);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_identifier_in_next_data() {
        let html = r#"<script id="__NEXT_DATA__">{"props":{},"page":"/plugins","buildId":"aB3-x_9","isFallback":false}</script>"#;
        assert_eq!(extract_build_id(html), Some("aB3-x_9"));
        assert_eq!(extract_build_id(r#"{"buildId" : "abc"}"#), Some("abc"));
    }

    #[test]
    fn skips_a_key_that_is_not_followed_by_a_string() {
        let html = r#"{"buildId":null,"x":{"buildId":"real"}}"#;
        assert_eq!(extract_build_id(html), Some("real"));
    }

    #[test]
    fn rejects_missing_or_path_like_identifiers() {
        assert_eq!(extract_build_id("<html></html>"), None);
        assert_eq!(extract_build_id(r#"{"buildId":""}"#), None);
        assert_eq!(extract_build_id(r#"{"buildId":"../../api"}"#), None);
        assert_eq!(extract_build_id(r#"{"buildId":"unterminated"#), None);
    }
}
