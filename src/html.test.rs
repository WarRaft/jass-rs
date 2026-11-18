use super::*;
use crate::parser::Parser;

fn html_for(src: &str) -> String {
    let program = Parser::parse_str(src).unwrap();
    render_html(&program, src, "demo.j")
}

#[test]
fn embeds_title_and_tree_data() {
    let html = html_for("function Foo takes nothing returns nothing\nendfunction");
    assert!(html.contains("demo.j"));
    assert!(html.contains("\"label\":\"Program\""));
    assert!(html.contains("id=\"ast-data\""));
    assert!(html.contains("id=\"L1\""));
    assert!(html.contains("tok-kw"));
}

#[test]
fn escapes_closing_script_tags_in_string_literals() {
    let html =
        html_for("function Foo takes nothing returns nothing\n    call P(\"</script>\")\nendfunction");
    assert!(!html.contains("</script>\")"));
    assert!(html.contains("<\\/script>"));
}

#[test]
fn highlights_keywords_strings_numbers_and_comments() {
    let html = html_for(
        "function F takes nothing returns nothing\nlocal integer x = 1 // hi \"there\"\nendfunction",
    );
    assert!(html.contains(r#"<span class="tok tok-kw" data-scol="1" data-ecol="6">local</span>"#));
    assert!(html.contains(r#"<span class="tok tok-num" data-scol="19" data-ecol="20">1</span>"#));
    assert!(html.contains(
        r#"<span class="tok tok-com" data-scol="21" data-ecol="34">// hi "there"</span>"#
    ));
}

#[test]
fn expression_node_spans_align_with_source_token_columns() {
    let html = html_for(
        "function Add takes integer a, integer b returns integer\n    return a + b\nendfunction",
    );
    // The `a` in `return a + b` is a Var node with span (2,12,2,13); the
    // highlighted source token for that same identifier must carry the
    // exact same columns, or clicking either side wouldn't find a match.
    assert!(html.contains(
        "\"label\":\"Var a\",\"kind\":\"expr\",\"line\":2,\"eline\":2,\"scol\":12,\"ecol\":13"
    ));
    assert!(html.contains(r#"<span class="tok tok-ident" data-scol="12" data-ecol="13">a</span>"#));
}
