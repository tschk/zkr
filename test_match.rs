fn main() {
    let kind = String::from("source");
    let s_rev = Some(1i64);
    let s_content = Some("test".to_string());

    let Some((target_revision, text)) = (match kind.as_str() {
        "source" => s_rev.zip(s_content),
        _ => None,
    }) else {
        return;
    };
    println!("{} {}", target_revision, text);
}
