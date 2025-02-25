pub async fn text_to_speech(text: &str, language: &str) -> String {
    println!("SIMULATION: Voice output '{}'", text);
    format!("[Voice in {}] {}", language, text)
}