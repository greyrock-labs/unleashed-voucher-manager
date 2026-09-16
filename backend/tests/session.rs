use backend::unleashed_api::extract_csrf_token;

#[test]
fn extracts_the_csrf_token_from_inline_script() {
    let html = r#"
        <script>
          var company = 'Ruckus Wireless';
        </script>
        <script>
var csfrToken = 'sBMp7znAEq';
</script>"#;
    assert_eq!(extract_csrf_token(html).as_deref(), Some("sBMp7znAEq"));
}

#[test]
fn returns_none_when_the_token_is_absent() {
    assert_eq!(extract_csrf_token("<html><body>nope</body></html>"), None);
}
