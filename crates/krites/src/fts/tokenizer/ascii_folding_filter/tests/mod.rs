//! Tests for the ASCII folding filter.

mod foldings_a_i;
mod foldings_j_s;
mod foldings_num_sym;
mod foldings_t_z;

use std::iter;

use super::to_ascii;
use crate::fts::tokenizer::{AsciiFoldingFilter, RawTokenizer, SimpleTokenizer, TextAnalyzer};

#[test]
fn test_ascii_folding() {
    assert_eq!(&folding_helper("Ràmon"), &["Ramon"]);
    assert_eq!(&folding_helper("accentué"), &["accentue"]);
    assert_eq!(&folding_helper("âäàéè"), &["aaaee"]);
}

#[test]
fn test_no_change() {
    assert_eq!(&folding_helper("Usagi"), &["Usagi"]);
}

fn folding_helper(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    TextAnalyzer::from(SimpleTokenizer)
        .filter(AsciiFoldingFilter)
        .token_stream(text)
        .process(&mut |token| {
            tokens.push(token.text.clone());
        });
    tokens
}

fn folding_using_raw_tokenizer_helper(text: &str) -> String {
    let mut token_stream = TextAnalyzer::from(RawTokenizer)
        .filter(AsciiFoldingFilter)
        .token_stream(text);
    token_stream.advance();
    token_stream.token().text.clone()
}

#[test]
fn test_latin1_characters() {
    let latin1_string = "Des mot clés À LA CHAÎNE À Á Â Ã Ä Å Æ Ç È É Ê Ë Ì Í Î Ï Ĳ Ð Ñ
               Ò Ó Ô Õ Ö Ø Œ Þ Ù Ú Û Ü Ý Ÿ à á â ã ä å æ ç è é ê ë ì í î ï ĳ
               ð ñ ò ó ô õ ö ø œ ß þ ù ú û ü ý ÿ ﬁ ﬂ";
    let mut vec: Vec<&str> = vec!["Des", "mot", "cles", "A", "LA", "CHAINE"];
    vec.extend(iter::repeat_n("A", 6));
    vec.extend(iter::repeat_n("AE", 1));
    vec.extend(iter::repeat_n("C", 1));
    vec.extend(iter::repeat_n("E", 4));
    vec.extend(iter::repeat_n("I", 4));
    vec.extend(iter::repeat_n("IJ", 1));
    vec.extend(iter::repeat_n("D", 1));
    vec.extend(iter::repeat_n("N", 1));
    vec.extend(iter::repeat_n("O", 6));
    vec.extend(iter::repeat_n("OE", 1));
    vec.extend(iter::repeat_n("TH", 1));
    vec.extend(iter::repeat_n("U", 4));
    vec.extend(iter::repeat_n("Y", 2));
    vec.extend(iter::repeat_n("a", 6));
    vec.extend(iter::repeat_n("ae", 1));
    vec.extend(iter::repeat_n("c", 1));
    vec.extend(iter::repeat_n("e", 4));
    vec.extend(iter::repeat_n("i", 4));
    vec.extend(iter::repeat_n("ij", 1));
    vec.extend(iter::repeat_n("d", 1));
    vec.extend(iter::repeat_n("n", 1));
    vec.extend(iter::repeat_n("o", 6));
    vec.extend(iter::repeat_n("oe", 1));
    vec.extend(iter::repeat_n("ss", 1));
    vec.extend(iter::repeat_n("th", 1));
    vec.extend(iter::repeat_n("u", 4));
    vec.extend(iter::repeat_n("y", 2));
    vec.extend(iter::repeat_n("fi", 1));
    vec.extend(iter::repeat_n("fl", 1));
    assert_eq!(folding_helper(latin1_string), vec);
}

#[test]
fn test_unmodified_letters() {
    assert_eq!(
        folding_using_raw_tokenizer_helper("§ ¦ ¤ END"),
        "§ ¦ ¤ END".to_string()
    );
}

#[test]
fn test_to_ascii() {
    let input = "Rámon".to_string();
    let mut buffer = String::new();
    to_ascii(&input, &mut buffer);
    assert_eq!("Ramon", buffer);
}
