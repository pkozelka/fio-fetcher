//! Utility functions and MIME type lookup for Google Drive/Sheets.

/// Map a filename extension to a MIME type.
#[allow(dead_code)]
pub fn mime_type_for_filename(filename: &str) -> &'static str {
    let lower = filename.to_lowercase();
    if lower.ends_with(".pdf") {
        "application/pdf"
    } else if lower.ends_with(".json") {
        "application/json"
    } else if lower.ends_with(".csv") {
        "text/csv"
    } else if lower.ends_with(".xml") {
        "application/xml"
    } else if lower.ends_with(".html") || lower.ends_with(".htm") {
        "text/html"
    } else if lower.ends_with(".txt") {
        "text/plain"
    } else if lower.ends_with(".zip") {
        "application/zip"
    } else {
        "application/octet-stream"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mime_types() {
        assert_eq!(mime_type_for_filename("test.pdf"), "application/pdf");
        assert_eq!(mime_type_for_filename("test.JSON"), "application/json");
        assert_eq!(mime_type_for_filename("test.csv"), "text/csv");
        assert_eq!(
            mime_type_for_filename("unknown.xyz"),
            "application/octet-stream"
        );
    }
}
