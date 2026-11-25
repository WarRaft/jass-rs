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

#### What this toolchain accepts at the top level

The grammar above is what the game reads. Source files here are built
into it, so the top level is more relaxed. Each relaxation that the game
itself would reject is reported by a rule in the lint group `pure`, off
by default: a project that must stay pure JASS turns it on with
`pure=error` in `[lint]` of `wc3.bni`.

- **`takes` and `returns` may be left out** of a `function` or `native`:
  left out, they mean `takes nothing` and `returns nothing`. Either one
  can be written alone. The game requires both (rule
  `implicit-signature`, group `pure`).

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
  (rule `implicit-keyword`, group `pure`).

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
  group `pure`); another of them that takes or returns something is a
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
  only inside `globals` (rule `global-outside-globals`, group `pure`).
- **Declarations may come after functions.** A variable, a `globals`
  block, a `native` or a `type` may follow a function. The game reads
  them only before the first function (rule `declaration-after-function`,
  group `pure`; an empty `globals` block declares nothing and isn't
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
| 6                       | `*` `/` (integer `/` truncates)                 |
| 7                       | unary `-`, `+`, `not`                           |

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

## Parser status

Supported: everything in the reference above **except**:

| not yet                                   | notes                                                   |
|-------------------------------------------|---------------------------------------------------------|
| `\r \b \f` escapes                        | decoded as the bare letter                              |
| locals-first rule                         | `local` is accepted anywhere in a body                  |
| type checking / declare-before-use        | no symbol or type resolution yet                        |
| vJASS member statement after `debug` without `set`/`call` | `debug set .x = 1` reads, `debug .x = 1` doesn't |
| JassHelper's preprocessor                 | run by a build (`wc3-build`'s `preprocess`); here a `//! textmacro` ... `//! endtextmacro` and a `//! novjass` ... `//! endnovjass` are one token each (an embedded block, as `//! zinc`), skipped by the parser and shown as a comment; a `//! runtextmacro` is a comment |

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
`LIBRARY_<name>` a boolean constant. A member (`a.b`, `a.b()`, `this`)
has the names it uses checked, and no type: what `a` is, and who may see
a private name, take vJASS's own resolution, which the build does.

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
- **Order, as the game reads it** (group `pure`): a call to a function
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
| *(none)*, or an unknown name | **plain JASS** (the default; `library` is a name, `//! zinc` a comment) |
| `// @dialect vjass`          | **vJASS**: vJASS keywords, `/* */` comments, embedded Zinc      |
| `// @dialect jass`           | plain JASS, stated explicitly                                   |

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
