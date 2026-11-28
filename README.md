# jass-rs

A parser and static analyzer for JASS, the Warcraft III scripting language,
built for WC3 modding tools. Language-agnostic pieces come from the shared
[`parser-rs`](../parser-rs) crate: spans, the lexer cursor, literal scanners,
the token stream, diagnostics, tree renderers and the HTML AST viewer.

- [The JASS language](#the-jass-language) — a syntax reference to build on
- [Strict and extended JASS](#strict-and-extended-jass) — the game's JASS and ours
- [Parser status](#parser-status) — what `jass-rs` handles today
- [CLI](#cli) · [Tests](#tests)

How the parser is built, and the rules every parser here follows, is in
the [`parser-rs` README](../parser-rs/README.md#principles).

---

## The JASS language

JASS ("Just Another Scripting Syntax") is the game's own scripting
language. Map scripts (`war3map.j`), the API declarations (`common.j`,
`blizzard.j`) and AI scripts (`.ai`) are all written in it. It's
statically typed, procedural, line-oriented and verbose. Blocks are
delimited by keyword pairs, not braces.

### Lexical structure

| element      | syntax                                                                                        |
|--------------|-----------------------------------------------------------------------------------------------|
| line breaks  | **significant** (strict JASS): a declaration or statement is one line and ends at its end; no `;`; a line break stands nowhere else but in a string or a rawcode. Extended JASS: anywhere it reads one way only (see below) |
| comments     | `// to end of line` only (no block comments in plain JASS; vJASS adds `/* */`)                |
| identifiers  | a letter, then letters/digits/`_`, not ending in `_`: `[A-Za-z]([A-Za-z0-9_]*[A-Za-z0-9])?`   |
| case         | everything is case-sensitive, keywords included                                               |
| integers     | decimal `42`, octal `017` (leading `0`), hex `0x1F` / `0X1F` / `$1F`                          |
| rawcodes     | `'hfoo'`: its bytes, 4 (or 1, `'a'`), up to the closing quote, packed big-endian into an integer; no escapes; any byte, a line break or an emoji's four too |
| reals        | `1.5`, `1.`, `.5`                                                                             |
| strings      | `"..."`: the quote takes everything up to the one closing it, line breaks included; escapes `\b \t \r \n \f \" \\`, any other an error. The game's text has its own codes: `\|cAARRGGBB` a color, `\|r` its end, `\|n` a line break, `TRIGSTR_n` a string of `war3map.wts` |
| constants    | `true`, `false`, `null`                                                                       |

**Keywords:** `function endfunction takes returns nothing native constant
type extends globals endglobals local array set call if then elseif else
endif loop endloop exitwhen return debug and or not true false null`.

### Types

- **Primitives:** `integer` (32-bit signed), `real` (32-bit float),
  `boolean`, `string`, `code` (a function reference), `handle` (the root of
  all object types).
- **Handle types** form a single-inheritance tree declared in `common.j`:

  ```jass
  type agent   extends handle
  type widget  extends agent
  type unit    extends widget
  ```

  A subtype can be used where its supertype is expected, not the other way
  round.
- **Conversions:** `integer` converts implicitly to `real`. Nothing else
  converts implicitly; use natives such as `R2I`, `I2S`, `S2I`. `null` is
  valid for handles, `string` and `code`.
- **Arrays** are one-dimensional and fixed-size (8192 elements; 32768
  since patch 1.29). They can't be passed, returned or initialized.

### File layout

All declarations must come before all functions:

```
file        := (type_decl | globals_block | native_decl)* function*
```

```jass
type mytype extends handle

globals
    constant integer MAX = 10          // constants must be initialized
    integer array    counters          // arrays can't be initialized
    unit             hero = null
endglobals

native GetHeroLevel takes unit whichHero returns integer
constant native GetTriggerUnit takes nothing returns unit
```

#### What this toolchain accepts at the top level

The grammar above is what the game reads. Source files here are built
into it, so the top level is more relaxed. Each relaxation that the game
itself would reject is reported by a rule in the lint group `strict`,
off by default in extended JASS (a project that wants its sources as the
game's turns it on with `strict=error` in `[lint]` of `wc3.bni`), and
always errors in strict JASS (see
[Strict and extended JASS](#strict-and-extended-jass)).

- **`takes` and `returns` may be left out** of a `function` or `native`:
  left out, they mean `takes nothing` and `returns nothing`. Either one
  can be written alone. The game requires both (rule
  `implicit-signature`, group `strict`).

  ```jass
  function Init                        // = takes nothing returns nothing
  endfunction
  function Count takes unit u          // = ... returns nothing
  endfunction
  function Seed returns integer        // = takes nothing returns integer
      return 42
  endfunction
  ```

- **`set`, `call` and `local` may be left out**: `x = 1`, `a[i] = x`,
  `Foo(x)` and `integer i = 0` as statements are an assignment, a call
  and a local. A function can't declare a global, so a declaration in
  one is a local, and at the top level a global. A statement that starts
  with a name is one of these, by what follows it (`=` or `[`: an
  assignment; `(`: a call; a name or `array`: a local, the first name
  its type), and a name alone is an error. So a value of `return` is on
  its line: `return` with a name on the next line returns nothing, and
  the name starts the next statement. The game requires the keyword
  (rule `implicit-keyword`, group `strict`).

  ```jass
  function Tick
      integer n = counter              // = local integer n = counter
      counter = n + 1                  // = set counter = n + 1
      DisplayTextToPlayer(p, 0, 0, "") // = call DisplayTextToPlayer(...)
  endfunction
  ```

- **`main` and `config` may be declared any number of times**, in any
  of the project's files, each taking and returning nothing: the build
  merges them into one, each one's statements in their order. So a
  piece of code sets itself up where it is, without one `main` that
  calls everything. The game reads one of each (rule `merged-function`,
  group `strict`); another of them that takes or returns something is a
  duplicate, as any other function's.

  ```jass
  trigger t = CreateTrigger()

  function main
      TriggerRegisterTimerEvent(t, 1.0, true)
  endfunction
  ```

- **Variables may be declared at the top level**, outside `globals` and
  anywhere among the functions. When the map is built they go into the
  `globals` block, so they're ordinary globals: same syntax, same rules,
  and duplicates are checked across both forms. The game reads variables
  only inside `globals` (rule `global-outside-globals`, group `strict`).
- **Declarations may come after functions.** A variable, a `globals`
  block, a `native` or a `type` may follow a function. The game reads
  them only before the first function (rule `declaration-after-function`,
  group `strict`; an empty `globals` block declares nothing and isn't
  reported).

  ```jass
  integer counter = 0                  // = a line in globals ... endglobals
  constant real PI = 3.14159

  function Tick takes nothing returns nothing
      set counter = counter + 1
  endfunction

  unit array heroes                    // after a function: still a global
  ```

- **Statements at the top level are recognized, then rejected.** `if`,
  `loop`, `set`, `call`, `exitwhen`, `return` and `local` are parsed in
  full, so the rest of the file still parses and highlights. Lint then
  reports an error asking to move the statement into a function. For
  `local`, it suggests dropping `local` to get a global.

  ```jass
  call Init()       // error: 'call' statement outside a function: move it into a function
  local integer i   // error: 'local' outside a function: drop 'local' to declare a global, ...
  ```

### Functions

```jass
[constant] function Name takes nothing returns nothing
[constant] function Name takes integer a, real b returns boolean
    local integer i = 0                // all locals first,
    local real array tmp               // before any statement
    ...statements...
endfunction
```

- `takes nothing` means no parameters; `returns nothing` means no return
  value. The game requires both written out; this toolchain lets you
  leave them out (see below).
- A function can only call functions declared **above** it. There are no
  forward declarations, and natives count as declared first.
- A non-`nothing` function must `return` a value on every path.

### Statements

| statement         | syntax                                                                       |
|-------------------|------------------------------------------------------------------------------|
| assignment        | `set x = expr`, `set arr[i] = expr`                                          |
| call              | `call Foo(args)`; the game reads a call as a statement only after `call`    |
| conditional       | `if cond then … [elseif cond then …]* [else …] endif`                        |
| loop              | `loop … endloop`, the only loop construct                                    |
| loop exit         | `exitwhen cond` (anywhere inside a `loop`, any number of times)              |
| return            | `return` / `return expr`                                                     |
| debug             | `debug` before `set`/`call`/`if`/`loop`: only runs in debug mode             |

`debug` before anything else is a parse error. Lint reads debug code
like any other (an empty branch inside is still reported, a variable
read there is still used), except that it can't end a loop or a
function: outside debug mode it isn't there, so an `exitwhen` or
`return` only inside `debug` doesn't count.

### Expressions

| precedence (low → high) | operators                                       |
|-------------------------|-------------------------------------------------|
| 1                       | `and`                                           |
| 2                       | `or`                                            |
| 3                       | `==` `!=`                                       |
| 4                       | `<` `>` `<=` `>=`                               |
| 5                       | `+` `-` (binary; `+` also concatenates strings) |
| 6                       | `*` `/` `%` (integer `/` truncates; `%` integers only) |
| 7                       | unary `-`, `+`, `not`                           |

Extended JASS (the default, see
[Strict and extended JASS](#strict-and-extended-jass)) also has `!` for
`not`, `&&` for `and` and `||` for `or`, at the same precedence, and
`set x += v`, `set x -= v`.

Operands are literals, variables, `arr[index]`, calls `Foo(args)`,
parenthesized expressions and code references `function Foo`. A code
reference must name a function that `takes nothing`.

**Watch out:** unlike C-family languages, JASS's `and` binds *looser*
than `or`, and Zinc's `&&`/`||` inherit this. So `false and true or true` means
`false and (true or true)`, which is `false`, not `true`. Add parentheses
when mixing the two. There is no modulo
operator; `ModuloInteger`/`ModuloReal` from `blizzard.j` fill that role.
`and`/`or` short-circuit.

### Example

```jass
globals
    integer array gCounters
endglobals

function SumCounters takes integer n returns integer
    local integer i = 0
    local integer total = 0
    loop
        exitwhen i >= n
        set total = total + gCounters[i]
        set i = i + 1
    endloop
    return total
endfunction

function Main takes nothing returns nothing
    if SumCounters(10) > 0 then
        call BJDebugMsg("positive")
    endif
endfunction
```

### vJASS

vJASS (JassHelper) is a preprocessor superset of JASS, compiled down to
plain JASS. A file is vJASS when its header says so (`// @dialect vjass`,
see [Dialects](#dialects-and-embedded-zinc)). On top of JASS it has `/* */`
comments, `//! zinc` blocks ([Zinc](../zinc-rs)), `.` and:

```jass
library Counters initializer Init requires Utils, optional TimerUtils  // also library_once; needs, uses
    globals
        private integer count = 0          // its library's only
        public constant integer MAX = 8    // Counters_MAX outside it
        integer array sized[100]           // a sized array
    endglobals

    private keyword Ahead                  // a name declared ahead
    function interface Callback takes integer x returns nothing
    type IntList extends integer array [16]     // a dynamic array type

    interface Shape
        method area takes nothing returns real
        method name takes nothing returns string defaults "shape"
    endinterface

    module Pooled
        static method get takes nothing returns thistype
            return thistype.allocate()
        endmethod
    endmodule

    struct Counter[20000] extends Shape    // or extends array, a struct
        integer value                      // an instance's
        static integer made = 0            // the struct's
        private real ratio
        readonly integer id                // set only inside the struct
        integer array list[4]
        implement Pooled                   // implement optional Extra

        static method create takes integer v returns thistype
            local thistype c = thistype.allocate()
            set c.list[1] = c.list[0] + 2
            set .id = thistype.made        // `.id` is `this.id`
            return c
        endmethod
        method operator [] takes integer i returns integer   // also []=, x, x=, <, >
            return .list[i]
        endmethod
        stub method onDestroy takes nothing returns nothing  // a child's may replace it
            call super.onDestroy()
        endmethod
    endstruct

    scope Inner initializer Start          // scopes nest, in libraries too
        private function Start takes nothing returns nothing
            call TimerStart(CreateTimer(), 1., false, function Counter.tick)
            static if LIBRARY_TimerUtils then   // kept or dropped by constants
                call Callback.evaluate(5)
            endif
        endfunction
    endscope
endlibrary
```

Modifiers (`private`, `public`, `static`, `readonly`, `stub`,
`delegate`, `constant`) come in any order before a declaration; `debug`
before a struct's member (`debug private boolean isNode`) makes it a
debug mode's only (`Modifiers::is_debug`). Requirements may be written
without commas, as JassHelper reads them and real libraries write them,
often a comment between each (`requires /* */ Ascii /* */ Table /* */
optional ErrorMessage`): no item of a library starts with a name, so the
next name or `optional` is the next one. `static if` goes around
declarations too, in a library or at the top (`TopLevel::StaticIf`), and
among a struct's, module's or interface's members
(`StructMember::StaticIf`; `ast::all_members` gives every branch's), as
it does around statements: every branch is read, which is there is the
build's. A static method may name a local `this` and set it (Table's
`local Table this = ...`, `set this = ...`). A `debug if` may be written a
line at a time, `debug` before its `else`, `elseif` and `endif` too, as
JassHelper takes a debug line.
`thistype` is the struct a member is in, `this` the instance (`.x` is
`this.x`), `super` the parent's; `Counter(5)` casts. A member of a
member and an element of one chain: `a.b[i].c()`. JassHelper's
preprocessor (`//! textmacro`, `//! runtextmacro`, `//! inject` ...)
works on text before any of this. `hook Name Target` (`Target` a
function or `S.m`): `TopLevel::Hook`.

---

## Strict and extended JASS

**Planned:** `Dialect` chosen by the language sections of `wc3.bni` and
a file's header (`[jass] strict`, `[vjass] enable`, `// @vjass.enable=0`)
in place of `[jass] dialect=` and `// @dialect`, which go away (no
compatibility kept at version 0); cJass's features as switches over any
dialect: see the [`wc3-rs` README](../wc3-rs/README.md#languages).

Two tasks need two readings. A map that's built must run, so what the
game would reject has to be found in it; and the sources a map is built
of are written as their authors prefer. Here **the build is the main
tool**: sources are written in extended JASS (or vJASS, Zinc ...), the
build makes the map's `war3map.j`, and that `war3map.j` goes through
strict JASS, to catch **the build's own mistakes**.

Every lint rule has its own name (`implicit-keyword`, ...; see the
[`wc3-rs` README](../wc3-rs/README.md#lint)). The rules that make the
reading the game's form the group **`strict`**: each finds what this
toolchain reads and the game doesn't.

- **Strict** (`Dialect::Strict`, `// @dialect strict`,
  `[jass] dialect=strict`, `jass_rs::parse`): exactly what the game
  reads, nothing more, for checking compiled code: the `war3map.j` a
  build made (the build checks it so), a map's `war3map.j` and
  `Scripts\war3map.j` (an earlier build's), its `.ai` and `.pld`, the
  game's scripts. By its path, such a file is strict whatever its
  header says: the game knows no `// @dialect`, a comment to it. What the game would reject is an error: the parser's,
  or a rule's of the group `strict`, whatever the project's `[lint]`
  says (`x = 1` without `set` parses, and is an error of
  `implicit-keyword`).
  **A strict file is one whole program.** The game has no imports: a
  map's script is one file, run with the game's `common.j` and
  `Blizzard.j` (`common.ai` for an AI script). So a strict file is read
  with the game's files only: it doesn't see its neighbours (its
  project's other files), and they don't see it.
  **A builder of one's own** makes a map's script of several files put
  together in an order: `jass_rs::strict::check(files, env)` reads them
  as that one file, glued in the order given, and says each problem at
  its own file and line (a "line N" in a message too, as `b.j:3`); `env`
  the game's declarations (`common.j`, `Blizzard.j`), before the script
  and not part of it. The CLI's `--strict` and `wc3-mcp`'s `code_check`
  (`strict`) are it. Order matters: the game knows a function only from
  its declaration down (`call-before-declaration`).
- **Extended** (`Dialect::Jass`, the default): strict JASS and what no
  strict program can mean otherwise. The rule: **no strict JASS can be
  written that extended JASS breaks or reads another way.** Each program
  the game reads, extended JASS reads the same; an addition only gives
  a meaning to what strict JASS rejects. A build writes the additions
  as the game's JASS.

So a new word can never be an addition: `integer scope = 1` is strict
JASS, and a keyword `scope` would break it (it does break vJASS, which
is why vJASS is a dialect a file opts into, not the default). A new
operator can be: `boolean b = !a` is an error in strict JASS, so nothing
the game reads changes when extended JASS reads it as `boolean b = not a`.

Extended JASS's additions (vJASS has them too, being built on it):

| extended          | strict                | notes                                             |
|-------------------|-----------------------|---------------------------------------------------|
| `!a`              | `not a`               | `!=` stays the comparison                         |
| `a && b`          | `a and b`             | `and`'s precedence: looser than `or`              |
| `a \|\| b`          | `a or b`              | so `a && b \|\| c` is `a and (b or c)`              |
| `set x += v`      | `set x = x + (v)`     | arrays and vJASS members too; without `set`, as `x = v` is (`implicit-keyword`) |
| `set x -= v`      | `set x = x - (v)`     | an index (`a[f()] -= 1`) is read twice, as Zinc's |

**Line breaks.** In strict JASS a line break ends a declaration or a
statement: each is one line (`if cond then`, `else`, `endif` too), and
two never share one. A line break stands nowhere else but in a string or
a rawcode, whose quote takes everything up to the closing one. The
parser records where a line may start (a declaration, a statement, a
block's end) and, in strict JASS, checks every token against that once
the program is read: one starting a line elsewhere is "a line break
inside a statement", one at such a place not starting its line "a second
statement on the line". Extended JASS reads a line break anywhere it
can only mean one thing, inside what can't have ended yet:

```jass
if
    a
    and
    b
then
    call F(
        a,
        b)
endif
```

Two places can be read two ways, and a line break ends the statement
there: `return`'s value starts on its line (the next line's `G()` is a
statement of its own), and in vJASS a `.` starting a line starts a
statement (`.x = 1`, a member of `this`), not a member of the line above.
Statements may share a line in extended JASS (`if a then call F() endif`).

`&&` and `||` keep JASS's precedence, not C's: they are only other
spellings of `and` and `or`, so mixing spellings never changes a
meaning. The AST doesn't tell the spellings apart (`UnaryOp::Not`,
`BinOp::And`, `BinOp::Or`); a compound assignment is `Stmt::Set` (or
`SetMember`) with `op: Some(BinOp::Add | BinOp::Sub)`. Names check its
type as the sum's (`set s += "a"` for a string, not `set b += 1` for a
boolean), and lint counts it as reading its target.

### The game's rules (pjass's)

The community checks scripts with pjass (`../pjass-master`); its checks
are lint rules here, each with its name (the [`wc3-rs`
README](../wc3-rs/README.md#lint) has them all). What breaks the game's
compiling or running the script is the group `strict`'s; what's likely a
mistake or a patch's limit, pjass's flags, the group `warnings`', each
set on its own and never forced. `tests/pjass.rs` reads pjass's own
tests (`should-check`, `should-fail`, a flag's by its rule) and checks
strict JASS reads them as pjass does, but for these, decided otherwise:

- **A name hidden.** A parameter or a local hiding a global variable is
  legal (`shadowed-global`, a warning, off). Named as a primitive
  (`local integer integer`) it's an error, as pjass has it
  (`hidden-type`); as a declared type or a function (`local effect
  effect`, which pjass lets be) the context tells them apart, but a
  beginner should see it: `shadowed-name`, a warning. A type named after
  a parameter or a local of its name hid it, or a global named as a
  type, the game can't read: `hidden-type` too. A build renames every parameter and local named as a global, a
  function, a type or `alias` (`x__l`), as extended JASS lets any name be
  hidden and inlining mustn't have a local take a global's place.
- **Parameters and locals of one name** are legal (`takes real B,
  integer B, integer B` and `local integer B` ... `local string B`; pjass
  says "already defined"). A local is, to the game, a parameter: declared
  by name, put on the function's stack as it's read. Reading a name, the
  game runs down the stack for the first that has it, from the top (from
  the right, as C++ pushes arguments): the last declared is found, an
  earlier one never. It doesn't stop the compiling, so it can't be an
  error: a slip or a typo, `duplicate-declaration`, a warning.
- **Strings and rawcodes.** A string's escapes are pjass's: `\b \t \r
  \n \f \" \\`, any other an error (a backslash before a line break too). A
  rawcode has none: the quote takes every byte up to the closing one (an
  emoji's four, a line break), one or four bytes. One an editor can't
  keep (a control character, a byte that isn't UTF-8, which reading the
  file as UTF-8 loses) is `unsafe-rawcode`'s, whose fix writes it as
  `0x...`, the same number (the editor's server takes lost bytes from the
  file).
- **The return bug** (pjass's `+rb`) was closed in 1.24: not read.
- `%` is an operator (integers only; `modulo-operator`, off, for patches
  before 1.29). `\r\n`, `\n` and a lone carriage return each break a
  line, in every dialect and everywhere, a `//` comment too
  (`parser_rs::lines`). pjass ends a comment only at `\n` (its tests
  `comment-strangeness.j`, `comments-only-end-in-lf.j`): taken for a
  mistake of pjass's author, nothing shows the game reads so; what the
  game does with a lone `\r` needs tests run in the game itself. A
  byte-order mark anywhere is whitespace; a NUL ends what
  the game reads: an error.

`lint::Options` gives the rules what's beyond the program: its source
(`number-overflow` reads how a number is written, World Editor's
`4294967296.0` aside: `war3map.w3s`'s "default" of a sound's real, 2^32,
as it writes `SetSoundPitch`; `unsafe-rawcode` a
rawcode's characters) and the string limits (`long-string`: 1023 bytes a
literal, 4092 a concatenation of literals; `[jass] string_limit` and
`concat_limit` in a project, 0 for none).

## Parser status

Supported: everything in the reference above **except**:

| not yet                                   | notes                                                   |
|-------------------------------------------|---------------------------------------------------------|
| locals-first rule                         | `local` is accepted anywhere in a body                  |
| type checking / declare-before-use        | no symbol or type resolution yet                        |
| vJASS member statement after `debug` without `set`/`call` | `debug set .x = 1` reads, `debug .x = 1` doesn't |
| JassHelper's preprocessor                 | run by a build (`wc3-build`'s `preprocess`); here a `//! textmacro` ... `//! endtextmacro` and a `//! novjass` (text after it a remark, as JassHelper takes it) ... `//! endnovjass` are one token each (an embedded block, as `//! zinc`), skipped by the parser and shown as a comment; a `//! runtextmacro` is a comment |

vJASS: everything in its reference above, in `Dialect::VJass` only (the
lexer turns its words and `.` on; in plain JASS `library` is a name). The
AST has `TopLevel::Library` (scopes too), `Struct`, `Interface`,
`Module`, `FunctionInterface`, `Keyword`; `Modifiers` on declarations;
`Stmt::SetMember`, `CallMethod`, `StaticIf`; `ExprKind::Member`,
`MethodCall`, `Index`, `This`, `ThisType`, `Super`. Lint checks the
functions in libraries and the methods of structs and modules as it does
functions (`implicit-keyword` for a member set or called without `set`
or `call`, which JassHelper requires too); the order rules
(`declaration-after-function`, `global-outside-globals`) leave vJASS's
declarations alone, which JassHelper gathers.

Every lint diagnostic names its rule. The rule ids, which rules are on
by default (`infinite-loop` isn't) and how a project switches them
(`[lint]` in `wc3.bni`) are in the [`wc3-rs` README](../wc3-rs/README.md#lint).

**Names** (`names`): `Declarations::of` is what a program declares
(functions and natives with their signatures, globals, types);
`names::check` resolves every name a program uses against its own and an
environment the caller gives (the game's `common.j` and `Blizzard.j`, the
project's other files: this crate reads no files), and reports
`unknown-name` (a function, variable or type declared nowhere),
`argument-count` (a call of the wrong arity, a code value of a function
that takes arguments), and types:

- `type-mismatch`: a value of the wrong type, as an argument, a value set
  or returned (or returned by a function that returns nothing), a
  condition (`if`, `elseif`, `exitwhen`: a boolean), an operator's side.
  A type goes where it or a type it extends is wanted (`unit` where
  `widget` or `handle`), an `integer` where a `real`, `null` where a
  handle, a `string` or `code`. Arithmetic takes numbers (an integer
  and a real make a real), `+` also joins strings, `<` and the like
  compare numbers, `==` compares numbers with numbers, any two handles,
  anything with its own type and anything with `null` (map optimizers
  write `x != null` of a boolean, and the game takes it).
- `array-access`: an array without its index, an index of what isn't an
  array (an index is an integer).
- `constant-assignment`: a constant set.
- `array-struct` (vJASS, JassHelper's manual, Array structs): a struct
  `extends array` has no instances allocated, so `lint` reports a
  member's default value, an array member and `onDestroy`, and `names`
  a call of `allocate`, of `create` not its own, of `destroy` or
  `deallocate`, on the struct by its name, `thistype`, `this` in its
  methods or a variable of its type, unless it (or a module it
  implements, `Declarations::modules`) has a method so named
  (`Declarations::array_structs`); and at each `implement`, what the
  module (and the modules it implements, in any file of the project)
  would give it that it can't have. The messages say what's wrong in
  their own words (`array_struct`, which `wc3-build` says too).

`names::comment_of` is the comment written for a declaration: the `//`
lines right above it (a rule line like `//=====` ends it: that's a
section's) and one after it on its line.

A value whose type can't be known (an unknown name, another language's)
isn't checked further, nor is what a wrong operation makes, so one
mistake is reported once. A local is seen in all of its function (locals
go anywhere). A name the environment knows without a signature (another
language's) is known, its calls not counted. `Blizzard.j`, `common.ai`,
the 41 AI scripts of the campaigns and the `war3map.j` of three maps
check clean but for one `null` given for an integer, which the game lets
through (an ignored test, `JASS_GAME` and `JASS_FILES`).

vJASS: what a library or scope declares is known by its name, a `public`
one also as `Scope_name`; a struct, interface, module, function
interface or `keyword` is a name known without its type (a struct's
values are integers to vJASS, so they go anywhere); `thistype` is a type,
`LIBRARY_<name>` and `DEBUG_MODE` boolean constants, `SCOPE_PREFIX` and
`SCOPE_PRIVATE` strings, `key` a type. A struct's values are integers: a
struct, `thistype`, an interface or a function interface adds and
compares as one. A member (`a.b`, `a.b()`) has the names it uses
checked, and no type: what `a` is takes vJASS's own resolution, which
the build does. In a struct's (module's, interface's) methods, `this` is
a `thistype` (set too), and its fields and methods are known without
`this.`: its own with their types, its modules' and its parent's
(`Declarations::structs`, `modules`) and what every struct has
(`allocate`, `deallocate`, `create`, `destroy` ...) without. A library's
names are its own first: two libraries' private `i` are two. A `static
if` of `LIBRARY_<name>`s (`not`, `and`, `or`) is checked as the build
keeps it: a branch of a library that isn't there (in the program and its
environment) left out; every branch of any other condition. Locals of a
`static if`'s branches are the function's. A code value where a game's
function (one the program doesn't declare) takes a `boolexpr` is
`Condition(function F)`, as JassHelper and the build make it.

Lint rules: duplicate globals (in `globals` and at the top level),
functions and locals, and statements outside a function (errors); unused
locals, `loop` without `exitwhen`, empty `then`/`else` branches, and a
missing `return` in a function that declares a return type (warnings).

**Symbols** (`symbols`, `semantic_tokens_and_symbols`): every name as a
`parser_rs::Occurrence`, recorded by the semantic-token classifier in the
same pass, so it works on half-typed code and always agrees with the
highlighting.

| names                                   | kind                           | scope                        |
|-----------------------------------------|--------------------------------|------------------------------|
| functions, natives                      | `Function`, `Native`           | project                      |
| globals, constants                      | `Global`, `Constant`           | project                      |
| types (`type`, not the primitives)      | `Type`                         | project                      |
| parameters, locals                      | `Parameter`, `Local`           | their signature (`Local(n)`, `n` counts signatures in the file) |
| vJASS libraries, scopes                 | `Library`, `Namespace`         | project                      |
| vJASS structs, interfaces, modules      | `Struct`, `Interface`, `Module` | project, or their library's when `private` |
| vJASS fields, methods                   | `Field`, `Method`              | member (`namespace`: the struct's name) |
| vJASS private names of a library        | as above                       | the library's (`Local(n)`, numbered with the signatures) |

vJASS (in `Dialect::VJass`: its words are keywords there alone, so plain
JASS is read as before): a library's or a scope's `private` names are in
its own local scope, and so are its uses of them, above the declaration
too (`initializer Init`), a scope's in a library seeing the library's;
`public` ones, and any other, are the project's (as `name`: the
`Scope_name` spelling isn't linked). A struct's (interface's, module's)
fields and methods are members, matched by name as Zinc's are (what a
value's struct is takes types); a name after `.` is one, and so is a
name a method uses without `this.` that its struct has: its own, its
modules' (`implement`) and its parent's (`extends`), the ones the file
declares. `requires A, optional B` (without commas, over lines) names
libraries; `implement M` a module; `S.m`, `function S.m` the struct;
`F.evaluate()`, `F.execute()`, `hook N F` functions.

A reference's kind is what it's classified as where it's used: a call is
a `Function` unless a `native` of that name is declared in the same file.
An embedded Zinc block's names come from `zinc-rs`
(`semantic_tokens_and_symbols_at`), its local scopes numbered on from the
file's own so they never collide.

### Planned

Not built yet; the modules are shaped for them (see the
[`wc3-mcp` README](../wc3-mcp/README.md#planned-jass)):

- **Name resolution**: an unknown function, variable or type is an error;
  a call with the wrong number of arguments too. It needs the project's
  names (the server's symbol index, the game's `common.j` and
  `Blizzard.j`), so the parser keeps every declaration and reference with
  its span and order.
- **Order, as the game reads it** (group `strict`): a call to a function
  declared below it, and a `local` after a statement. Off for sources,
  where the build sorts functions and locals go anywhere; on for a map's
  `war3map.j`.
- **Types**, after names: arguments, `set`, `return`, conditions, the
  `handle` hierarchy from `common.j`.
- **Comments above declarations** kept with them, so natives can be
  listed with what a `common.j` says about them.

## Dialects and embedded Zinc

Each file declares its dialect in its header, so a project can mix plain
JASS, vJASS and (later) other dialects:

| header                       | dialect                                                         |
|------------------------------|-----------------------------------------------------------------|
| *(none)*, or an unknown name | **extended JASS** (the default; `library` is a name, `//! zinc` a comment) |
| `// @dialect vjass`          | **vJASS**: extended JASS, vJASS keywords, `/* */` comments, embedded Zinc |
| `// @dialect jass`           | extended JASS, stated explicitly                                |
| `// @dialect strict`         | **strict JASS**: exactly the game's (compiled code)             |

Only the header (leading blank and comment lines) is read. The name is
case-insensitive. A Chinese JASS dialect is planned as another
`Dialect` with its own options.

- **Projects.** In the editor, a file without a header gets its
  project's default dialect instead: `[jass] dialect=vjass` in
  `wc3.bni` (see the [`wc3-rs` README](../wc3-rs/README.md#projects)).
  A header always wins over the project.
- **API.** `jass_rs::parse_file(src)` honours the header (`Dialect::detect`);
  `Dialect::declared(src)` is the header alone, `None` when it names no
  known dialect.
  `jass_rs::parse(src)` is always strict JASS (for the game's scripts
  and a build's output), and
  `jass_rs::parse_with(src, dialect)` forces a dialect.
- **vJASS** is the same parser with a bigger keyword set
  (`VJASS_KEYWORDS`), `/* */` comments, and **embedded Zinc**.

```jass
// @dialect vjass
globals
    integer gCount = 0
endglobals

//! zinc
library Counter {
    public function Bump(integer by) -> integer { gCount += by; return gCount; }
}
//! endzinc

function Main takes nothing returns nothing
    call Bump(1)
endfunction
```

- **Lexer.** It turns each `//! zinc` … `//! endzinc` block into one
  token.
- **Parser.** At top level, the parser hands the block to
  `zinc_rs::parse_at` and resumes with the next JASS token. The block
  becomes `TopLevel::Zinc(ZincBlock { program, open, close })`.
- **Positions.** All positions inside it are file-absolute.
- **Delegation.** Lint, the AST tree, `find_node_at` (which returns
  `AstRef::Zinc`) and semantic highlighting all delegate to `zinc-rs`.
- **Restrictions.** A block inside a function is a parse error. An
  unterminated block is an error at its `//! zinc` line.
- **Plain JASS.** Here `//! zinc` is just a comment.

The VS Code extension honours each file's header as well. See the
[`parser-rs` design notes](../parser-rs/README.md#4-embedded-languages-one-token-then-hand-over-control)
for how embedding works in general.

## Formatting

`format::format(src, dialect, unit, lines)`: each line moved left or right
to its level, never a line added, removed or joined (see the
[`parser-rs` README](../parser-rs/README.md#13-formatting-only-moves-lines)).
A line starting with `function`, `globals`, `if`, `loop` opens a block
and its `end...` closes it; `else` and `elseif` stand at their `if`.
vJASS adds `library` (`library_once`), `scope`, `struct`, `interface`,
`module`, `method` and `static if`. Modifiers before the keyword
(`private`, `public`, `static`, `constant`, `stub`, `debug`) don't count,
and what has no body opens nothing: `function interface`, a method of an
interface. A closing keyword with nothing to close stays at its level.
A `//! zinc` block is formatted as Zinc at its directives' level; a
`//! textmacro`, `//! novjass` or `//! inject` block's lines (text, not
code of their place) move with their opening directive. The lines a
string goes on into stay. The game's `Blizzard.j`, formatted with four
spaces, changes only its tabs and its blank lines' spaces.

## CLI

```sh
cargo run -- file.j            # lint
cargo run -- --ast file.j      # indented AST tree
cargo run -- --dot file.j      # Graphviz DOT
cargo run -- --html file.j > file.ast.html && open file.ast.html
cargo run -- --vjass file.j    # force vJASS (default: the file's `// @dialect` header)
cargo run -- --jass file.j     # force plain JASS
# strict JASS: the files as one script, glued in this order; names over --with's
cargo run -- --strict --with common.j --with Blizzard.j head.j lib.j main.j
```

## Tests

```sh
cargo test
UPDATE_SNAPSHOTS=1 cargo test --test html_snapshot   # regenerate HTML goldens
```
