use {super::*, ord::subcommand::parse::Output, ord::Object};

#[test]
fn hash() {
  assert_eq!(
    CommandBuilder::new("parse 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
      .output::<Output>(),
    Output {
      object: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        .parse::<Object>()
        .unwrap(),
    }
  );
}

#[test]
fn unrecognized_object() {
  CommandBuilder::new("parse A")
    .stderr_regex(r#"error: .*: unrecognized object\n.*"#)
    .expected_exit_code(2)
    .run();
}
