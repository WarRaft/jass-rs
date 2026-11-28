use super::*;
use parser_rs::apply_edits;

fn fmt(src: &str, dialect: Dialect) -> String {
    let out = apply_edits(src, &format(src, dialect, "    ", 1..=usize::MAX));
    assert_eq!(line_count(&out), line_count(src), "a formatter keeps every line");
    out
}

#[test]
fn indents_functions_ifs_and_loops() {
    let src = "\
globals
integer x = 0
        endglobals
  function F takes nothing returns nothing
local integer i = 0
loop
exitwhen i > 10
if i == 1 then
call DoNothing()
    elseif i == 2 then
set x = 1
        else
// a comment
set x = 2
endif
set i = i + 1
endloop
     endfunction
";
    let want = "\
globals
    integer x = 0
endglobals
function F takes nothing returns nothing
    local integer i = 0
    loop
        exitwhen i > 10
        if i == 1 then
            call DoNothing()
        elseif i == 2 then
            set x = 1
        else
            // a comment
            set x = 2
        endif
        set i = i + 1
    endloop
endfunction
";
    assert_eq!(fmt(src, Dialect::Jass), want);
}

#[test]
fn a_string_going_on_stays() {
    let src = "function F takes nothing returns nothing\ncall BJDebugMsg(\"a\n  b\")\nendfunction\n";
    assert_eq!(
        fmt(src, Dialect::Jass),
        "function F takes nothing returns nothing\n    call BJDebugMsg(\"a\n  b\")\nendfunction\n"
    );
}

#[test]
fn vjass_blocks_and_modifiers() {
    let src = "\
library L initializer Init requires R
private keyword K
function interface Callback takes nothing returns nothing
interface I
method m takes nothing returns nothing
endinterface
struct S extends I
static integer n = 0
private static method create takes nothing returns thistype
static if DEBUG then
debug call BJDebugMsg(\"x\")
endif
return 0
endmethod
endstruct
module M
endmodule
private function Init takes nothing returns nothing
/* a
   b */
endfunction
endlibrary
";
    let want = "\
library L initializer Init requires R
    private keyword K
    function interface Callback takes nothing returns nothing
    interface I
        method m takes nothing returns nothing
    endinterface
    struct S extends I
        static integer n = 0
        private static method create takes nothing returns thistype
            static if DEBUG then
                debug call BJDebugMsg(\"x\")
            endif
            return 0
        endmethod
    endstruct
    module M
    endmodule
    private function Init takes nothing returns nothing
        /* a
           b */
    endfunction
endlibrary
";
    assert_eq!(fmt(src, Dialect::VJass), want);
}

#[test]
fn library_is_a_name_in_plain_jass() {
    let src = "function library takes nothing returns nothing\nendfunction\n";
    assert_eq!(fmt(src, Dialect::Jass), src);
}

#[test]
fn embedded_zinc_is_formatted_as_zinc() {
    let src = "\
  //! zinc
library A {
function f() {
x = 1;
}
}
    //! endzinc
function G takes nothing returns nothing
endfunction
";
    let want = "\
//! zinc
library A {
    function f() {
        x = 1;
    }
}
//! endzinc
function G takes nothing returns nothing
endfunction
";
    assert_eq!(fmt(src, Dialect::VJass), want);
}

#[test]
fn a_textmacro_moves_with_its_directive() {
    let src = "\
library L
//! textmacro T takes NAME
  function $NAME$ takes nothing returns nothing
  endfunction
//! endtextmacro
endlibrary
";
    let want = "\
library L
    //! textmacro T takes NAME
      function $NAME$ takes nothing returns nothing
      endfunction
    //! endtextmacro
endlibrary
";
    assert_eq!(fmt(src, Dialect::VJass), want);
}

#[test]
fn an_unclosed_block_and_a_stray_end() {
    assert_eq!(fmt("endif\nfunction F\ncall X()\n", Dialect::Jass), "endif\nfunction F\n    call X()\n");
}

#[test]
fn an_unterminated_zinc_block_goes_to_the_end() {
    assert_eq!(fmt("//! zinc\nlibrary A {\nx;\n", Dialect::VJass), "//! zinc\nlibrary A {\n    x;\n");
}
