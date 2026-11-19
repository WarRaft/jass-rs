# jass-rs

A parser and static analyzer for JASS, the Warcraft III scripting language,
built for WC3 modding tools. Language-agnostic pieces come from the shared
[`parser-rs`](../parser-rs) crate: spans, the lexer cursor, literal scanners,
the token stream, diagnostics, tree renderers and the HTML AST viewer.

- [The JASS language](#the-jass-language) — a syntax reference to build on
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
| line breaks  | **significant**: every declaration/statement ends at end of line; no `;`                      |
| comments     | `// to end of line` only (no block comments in plain JASS; vJASS adds `/* */`)                |
| identifiers  | a letter, then letters/digits/`_`, not ending in `_`: `[A-Za-z]([A-Za-z0-9_]*[A-Za-z0-9])?`   |
| case         | everything is case-sensitive, keywords included                                               |
| integers     | decimal `42`, octal `017` (leading `0`), hex `0x1F` / `0X1F` / `$1F`                          |
| rawcodes     | `'hfoo'`: 4 chars packed big-endian into an integer (also a 1-char form `'a'`)                |
| reals        | `1.5`, `1.`, `.5`                                                                             |
| strings      | `"..."` with escapes `\n \t \r \b \f \" \\`; single-line                                      |
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
  value.
- A function can only call functions declared **above** it. There are no
  forward declarations, and natives count as declared first.
- A non-`nothing` function must `return` a value on every path.

### Statements

| statement         | syntax                                                                       |
|-------------------|------------------------------------------------------------------------------|
| assignment        | `set x = expr`, `set arr[i] = expr`                                          |
| call              | `call Foo(args)`; a call is never a statement without `call`               |
| conditional       | `if cond then … [elseif cond then …]* [else …] endif`                        |
| loop              | `loop … endloop`, the only loop construct                                    |
| loop exit         | `exitwhen cond` (anywhere inside a `loop`, any number of times)              |
| return            | `return` / `return expr`                                                     |
| debug             | `debug` before `set`/`call`/`if`/`loop`: only runs in debug mode             |

### Expressions

| precedence (low → high) | operators                                       |
|-------------------------|-------------------------------------------------|
| 1                       | `and`                                           |
| 2                       | `or`                                            |
| 3                       | `==` `!=`                                       |
| 4                       | `<` `>` `<=` `>=`                               |
| 5                       | `+` `-` (binary; `+` also concatenates strings) |
| 6                       | `*` `/` (integer `/` truncates)                 |
| 7                       | unary `-`, `not`                                |

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

vJASS (JassHelper) is a preprocessor superset: `library`/`scope`,
`private`/`public`, `struct`/`interface`/`module`, `method`, `static if`,
`/* */` comments, `//! textmacro`, `//! zinc` blocks ([Zinc](../zinc-rs)),
and more. It compiles down to plain JASS.

---

## Parser status

Supported: everything in the reference above **except**:

| not yet                                   | notes                                                   |
|-------------------------------------------|---------------------------------------------------------|
| `constant function`                       | only `constant native` is accepted                      |
| `debug` statements                        | the keyword isn't recognized                            |
| `$1F` hex, octal `017`                    | `017` is read as decimal 17                             |
| `'a'` (1-char rawcode), `1.` / `.5` reals |                                                         |
| `\r \b \f` escapes                        | decoded as the bare letter                              |
| locals-first rule                         | `local` is accepted anywhere in a body                  |
| declarations-before-functions order       | top-level items may be interleaved                      |
| type checking / declare-before-use        | no symbol or type resolution yet                        |
| vJASS                                     | `Dialect::VJass` = JASS + vJASS keywords + `/* */` + `//! zinc` blocks; `library`/`struct`/... grammar not yet |

Lint rules: duplicate globals, functions and locals (errors); unused
locals, `loop` without `exitwhen`, empty `then`/`else` branches, and a
missing `return` in a function that declares a return type (warnings).

## Dialects and embedded Zinc

Each file declares its dialect in its header, so a project can mix plain
JASS, vJASS and (later) other dialects:

| header                       | dialect                                                         |
|------------------------------|-----------------------------------------------------------------|
| *(none)*, or an unknown name | **plain JASS** (the default; `library` is a name, `//! zinc` a comment) |
| `// @dialect vjass`          | **vJASS**: vJASS keywords, `/* */` comments, embedded Zinc      |
| `// @dialect jass`           | plain JASS, stated explicitly                                   |

Only the header (leading blank and comment lines) is read. The name is
case-insensitive. A Chinese JASS dialect is planned as another
`Dialect` with its own options.

- **API.** `jass_rs::parse_file(src)` honours the header (`Dialect::detect`).
  `jass_rs::parse(src)` is always plain JASS, and
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

## CLI

```sh
cargo run -- file.j            # lint
cargo run -- --ast file.j      # indented AST tree
cargo run -- --dot file.j      # Graphviz DOT
cargo run -- --html file.j > file.ast.html && open file.ast.html
cargo run -- --vjass file.j    # force vJASS (default: the file's `// @dialect` header)
cargo run -- --jass file.j     # force plain JASS
```

## Tests

```sh
cargo test
UPDATE_SNAPSHOTS=1 cargo test --test html_snapshot   # regenerate HTML goldens
```
