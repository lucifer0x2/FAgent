# OCD source for apply_batch

Send `source` (OCD v1) alone or before `operations`: one atomic batch (`strict`, `dryRun`,
idempotency unchanged). Errors on source give `line`/`column` (`SOURCE_PARSE_FAILED` adds
`suggestion`); a JSON operation's `opIndex` counts in `operations`.
Big batches: `apply_batch_file` an inbox `x.ocd`.

```
line  := indent (node | directive) [// comment]   indent = 2 spaces
node  := kind[#id][.styleId] {attr | flag | WxH} ["text"]
kind  := page | frame | group | rect | text | image | comp | use <componentId>
attr  := key=value   value: number | fill | hug | 50% | #hex | $token | a,b,c
                            | "json string" | {json} | [json]   (no spaces outside quotes)
flag  := row | col | free | wrap | clip (clip=true)
@token id value [type=] [name=] {theme=value}   @style id [name=] {attr}
@set id {attr}   @del id (with its notes)   @move id parentId [index]   @order id index
@note nodeId "text" [id=] [author=]   @reply noteId "text" [author=]   @resolve noteId
@note-set noteId ["text"] [resolved=] [node=]   @note-del noteId
@in parentId   (indented new nodes with #id under an existing node)
```

| Shorthand | Writes |
| --- | --- |
| `400x44`, `w=`, `h=`, `r=` | `width`, `height`, `radius` |
| `row`/`col`/`free`, `wrap` | `layout.mode`, `layout.wrap` |
| `p=0,12`, `gap=`, `align=`, `justify=between` | `layout.padding/gap/align/justify` (`space-between`) |
| `.body`, `$ink` | `styleId`, `{"tokenId":"ink"}` |
| `stroke=#E5E7EB,2,inside` | `stroke {color,width,align}` |
| `size=24/32`, `font=`, `family=`, `weight=` | `fontSize`/`lineHeight`, `fontId`, `fontFamily`, `fontWeight` |
| `comp#input name=Input` | frame + `component {name}` (name: ID) |
| `use#a input ph="x"`, `ph.r=8` (any node key) | instance `componentId`, `overrides {ph:{text,radius}}` |
| `key={json}`, `props={json}`, `index=2` | raw value; props merged last; create index |

Every catalog key (skill://fagent/props.md) works as written; later attributes win.
On `use` a non-instance key (on `@set` a non-prop key) names a master descendant (string:
its `text`; `{json}`: its override); node props, and descendants missing from a `comp` in
the source, fail. A trailing string is a text node's `text`, else `name`. `@set` replaces whole values (`layout={…}`, `stroke=#000,1,inside`); partial writes
(flags, `gap=`, `layout.gap=`, `stroke=#000`, `ph.fill=`, `ph="x"`, `key=null`) send
`update_node` `merge:true`, a JSON merge patch keeping the rest (`null` deletes;
`{tokenId}`, `{styleId}`, gradients replace).

Defaults: root pages need `#id`; other nodes get `<nearest #id>-<kind>-<hash of
parent, kind, text/name>`, returned as `autoIds` {line: id}: give nodes to edit an `#id`.
Root frames without `h` hug in `row`/`col`. Text width: `"fill"` in a start-aligned `col`
of non-hug width, unset under `align=stretch`, else `"hug"`; height `"hug"` (unset in a
stretched row); with breakpoints only agreed sizes; none under `@in`. Token/style `name`:
the ID; token `type`: `color` for `#hex`, `dimension` for numbers, else required; `$x` is
an alias. A style of only `fill` is a fill style, else text.

<!-- ocd-sample -->
```ocd
@token ink #111827
@token muted #6B7280
@token pri #2563EB
@token line #D1D5DB
@style h1 font=font_bundled size=24/32 fill=$ink
@style body font=font_bundled size=14/20 fill=$muted

page#login
  comp#input name=Input 336x44 r=8 stroke=$line row p=0,12 align=center
    text#ph .body "Placeholder"
  frame#screen 1280x800 fill=#F3F4F6 col align=center justify=center
    frame#card w=400 h=hug fill=#FFFFFF r=16 stroke=#E5E7EB col p=32 gap=16 align=stretch
      text#title .h1 "Sign in"
      text .body "Enter your email and password."
      use#email input ph="email@example.com"
      use#pass input ph="Password"
      frame#btn h=44 r=8 fill=$pri row align=center justify=center
        text .body fill=#FFFFFF "Continue"
  @note card "Sign-in card: stack column." author=demo
```

Edit (expectedRevision of your read):

<!-- ocd-edit -->
```ocd
@set title text="Welcome back" size=28
@set card gap=24
@del pass
```
