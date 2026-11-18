#[test]
fn launch_options_are_public() {
    let options = tbr_plus::LaunchOptions {
        terminal: true,
        filename: Some("book.txt".to_owned()),
    };
    assert!(options.terminal);
    assert_eq!(options.filename.as_deref(), Some("book.txt"));
}
