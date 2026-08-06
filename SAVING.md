# Saving & Loading Maps — A Beginner's Guide

## Who this guide is for

You can already build and run **jade-ed** (the map editor) and **jade** (the
renderer/game). You can draw sectors, walls, and obstacles, and you have a map
that shows up in both apps. What you want now is:

1. A way to **save** the map you made in jade-ed to a file.
2. A way for **jade** to **read** that file and render your map instead of the
   built-in test map.

This guide assumes you know how to use the editor, but *not* a lot about Rust
or how Rust projects are put together. So we start slow: what a crate is, what
`use` and `#[derive]` do, what serde is, and only then the map itself.

By the end you will understand the whole pipeline and be able to implement it
in about a hundred lines per project. You will also understand **how to use the
two new crates you add** — `serde` and `serde_json` — well enough to apply them
elsewhere.

The guide is *conceptual and thorough*: the snippets are complete enough to
type in, but each one is followed by an explanation so you learn why it works,
not just that it does.

### The roadmap

| Part | What it covers |
|---|---|
| 0 | The tools: Cargo, crates, modules, `use`, `#[derive]`, basic types |
| 1 | The map data model, explained gently |
| 2 | Why we can't just write the map to disk |
| 3 | Adding and using the `serde` + `serde_json` crates |
| 4 | The design: a "disk model" and two conversions |
| 5 | Converting `Map → SaveMap` (handle → path) |
| 6 | Converting `SaveMap → Map` (path → handle) |
| 7 | Saving in jade-ed |
| 8 | Loading in jade-ed |
| 9 | Loading in jade (the renderer) |
| 10 | Keeping the two sides in sync |
| 11 | Gotchas |
| 12 | Verifying it works |
| 13 | Going further: a shared crate |
| Glossary | Every term, one line |
| TL;DR | The whole thing in bullets |

---

## Part 0 — The tools

Before we touch any code, let's make sure the vocabulary is solid. Everything
in this part is the "air you breathe" when writing Rust, and the rest of the
guide leans on it.

### 0.1 What a Rust project is

A Rust project (a "package") is a folder with two important things:

- **`Cargo.toml`** — a small text file describing the package: its name, its
  version, and a list of `[dependencies]`. This is the project's "shopping
  list": the crates it uses.
- **`src/`** — the folder with your code. In this repo it holds `.rs` files
  like `src/map.rs`, `src/ui.rs`, `src/tools.rs`, and so on.

Other folders exist but aren't your code: `target/` is where the compiler
puts its output, and `Cargo.lock` is a machine-generated record of *exactly*
which versions of every dependency were used (you never edit it by hand).

The two commands you care about:

```text
cargo build    # compile the project (catches errors, makes a binary)
cargo run      # compile (if needed) and run the binary
cargo check    # compile-check only, faster; good for quick feedback
```

### 0.2 What a crate is

A **crate** is a library or program written in Rust. It's the unit of code that
gets compiled. The Rust ecosystem shares code by publishing crates, and you use
someone else's crate by listing it as a **dependency** in your `Cargo.toml`.

Open `jade-ed/Cargo.toml`:

```toml
[dependencies]
bevy = { version = "0.19", features = ["dynamic_linking", "debug", "bevy_dev_tools"] }
bevy_egui = "0.41.1"
bevy-inspector-egui = "0.37"
```

This says: "this project uses the `bevy` crate (version 0.19, with some extra
features), the `bevy_egui` crate, and the `bevy-inspector-egui` crate." When you
add `serde` and `serde_json`, they become lines in this same list.

When you run `cargo build`, Cargo reads this list, downloads the crates, and
records the exact versions in `Cargo.lock`. That's why `Cargo.lock` already
contains `serde`, `serde_json`, and `glam` even though your projects don't use
them yet — bevy and its friends use them internally.

> **Key idea:** "Adding a crate" = adding one line to `Cargo.toml`. "Using a
> crate" = referring to it from your code with `use`. That's it.

### 0.3 Modules and `mod`

Your code is split into files. Rust connects those files with `mod`
declarations. Look at `jade-ed/src/main.rs`:

```rust
mod editor;
mod height_handles;
mod map;
mod map_gizmos;
mod map_handles;
mod map_preview;
mod mode;
mod scene;
mod tools;
mod ui;
mod viewport;
mod picking;
```

Each `mod something;` tells Rust "there is a module named `something` living in
`src/something.rs`". So `mod map;` means "my map code is in `src/map.rs`".

Two layouts are in use in this repo:

- **`jade-ed`**: flat files, `src/map.rs`, declared in `src/main.rs`.
- **`jade`**: a folder module, `src/map/mod.rs`, and the file declares its own
  submodules. Look at the top of `jade/src/map/mod.rs`:

  ```rust
  pub mod relative_map;
  ```

  That tells Rust "there is a submodule `relative_map` in
  `src/map/relative_map.rs`".

When we add a `save.rs` file later, this is exactly where we declare it: in
jade-ed it's `src/save.rs` + `mod save;` in `main.rs`; in jade it's
`src/map/save.rs` + `pub mod save;` in `map/mod.rs`.

> **Key idea:** `mod foo;` = "attach file `foo.rs` to my program under the name
> `foo`". It's how files become part of the same program.

### 0.4 What `use` does

`use` brings a name into scope so you can type a short name instead of a long
path. Bevy re-exports common things through its `prelude`:

```rust
use bevy::prelude::*;   // bring in *everything* bevy recommends
```

That's why the code can say `Vec2`, `Res<Map>`, `Commands` and so on without
long paths. Without `use`, you'd have to write `bevy::prelude::Vec2` every time.

When we add serde we'll write:

```rust
use serde::{Serialize, Deserialize};      // bring in two trait names
use serde_json::to_string_pretty;         // (example) bring in a function
```

`use` doesn't *copy* anything; it just makes a short name available. The
compiler will complain if you `use` a name you never use — that's a hint to
clean up, not an error that means your program is broken.

### 0.5 Traits and `#[derive(...)]`

A **trait** is a set of behaviors (methods) a type promises to have. For
example, `std::fmt::Display` is the trait for "can be turned into text".
`std::fmt::Debug` is "can be printed for debugging".

Writing these implementations by hand is repetitive. Rust gives you a
shortcut: **derive macros**. You write `#[derive(...)]` above a type and the
compiler *writes the implementation for you*:

```rust
#[derive(Debug)]
struct MyPoint {
    x: f32,
    y: f32,
}
```

Now `MyPoint` can be printed with `{:?}`. You didn't write any code for that;
`Debug` is *derived*.

serde's two traits — `Serialize` and `Deserialize` — are exactly the same idea:

```rust
#[derive(Serialize, Deserialize)]
struct MyPoint { x: f32, y: f32 }
```

This generates, automatically, code that can convert `MyPoint` into a
serialized form and back. You never write that code; the derive macro does.

> **Key idea:** `#[derive(TraitA, TraitB)]` = "compiler, please implement
> TraitA and TraitB for this type using its fields." We will use this on every
> struct in our save file format.

### 0.6 The types you'll meet

A quick tour of the vocabulary used throughout this guide:

- **`f32`** — a 32-bit floating point number (a decimal like `0.0` or `20.0`).
  All map coordinates and heights are `f32`.
- **`usize`** — a non-negative whole number (used for indices and ids).
- **`String`** — a growable piece of text, owns its bytes. `"texture.png".to_string()`.
- **`&str`** — a *borrowed* view of text. Literals like `"texture.png"` are
  `&str`. Converting: `&str` → `String` with `.to_string()`; `String` → `&str`
  with `.as_str()` (usually implicit).
- **`Vec<T>`** — a list of `T`. `Vec<f32>` is a list of floats.
  `.iter()`, `.len()`, `.push()`, `.collect()`.
- **`Option<T>`** — "maybe a `T`". `Some(x)` or `None`. The map uses this for
  portal backsides: `back_side_def: Option<SideDef>` is `None` for a solid wall
  and `Some(...)` for a portal. `map_or(default, f)` means "if Some, apply f;
  if None, use default".
- **`Result<T, E>`** — "a `T` or an error `E`". `Ok(x)` or `Err(e)`. File
  reads/writes return this. The `?` operator unwraps `Ok` and *returns early*
  on `Err`. `map_err(|e| ...)` turns one error type into another.
- **`Handle<Image>`** — bevy's reference to a loaded image. We'll meet it in
  Part 1; it's the reason this guide exists.
- **`Vec2`** — bevy's 2D vector (`bevy::math::Vec2`, re-exported in the
  prelude). It's a `glam` type under the hood. Holds `x` and `y`. In this
  project `Vec2` means "(x, z)" — the horizontal plane the map lives on.

---

## Part 1 — What we're saving (the map, explained gently)

Now let's look at the actual thing we want to save: the `Map`. It lives in
`jade-ed/src/map.rs` and, duplicated, in `jade/src/map/mod.rs`. Understanding
it is 90% of this task.

### 1.1 The `Map` resource

```rust
#[derive(Resource, Default, Clone)]
pub struct Map {
    pub vertices: Vec<Vec2>,
    pub sectors: Vec<Sector>,
}
```

Two fields:

- **`vertices`** — a big list of positions. This is called an *indexed vertex
  pool*: walls don't store positions, they store *indices* into this list.
- **`sectors`** — the rooms/areas of the map, each with walls and obstacles.

In bevy, `#[derive(Resource)]` marks `Map` as a globally-accessible resource
that any system can read with `Res<Map>` or write with `ResMut<Map>`. That's
how both apps share it everywhere.

### 1.2 Why an "indexed vertex pool"?

Two walls that meet at a corner must share that corner's *exact* position, or
they won't line up. The map guarantees this by **de-duplicating** positions:
there is a helper `add_vertex` (`jade-ed/src/map.rs:731`) that only adds a
position if it isn't already in the pool, and returns its index either way:

```rust
fn add_vertex(pool: &mut Vec<Vec2>, pos: Vec2) -> usize {
    if let Some(idx) = pool.iter().position(|&v| v == pos) {
        idx          // already there — return the existing index
    } else {
        pool.push(pos);
        pool.len() - 1
    }
}
```

So if sector A and sector B share an edge, both of their walls can point at the
*same* pooled vertices. This matters enormously for portals (next section), and
it matters for saving: **the indexed pool is exactly the shape the data should
take on disk.** We keep indices, not raw positions.

### 1.3 `Sector`

```rust
pub struct Sector {
    pub walls: Vec<LineDef>,
    pub obstacles: Vec<Obstacle>,
    pub floor_height: f32,
    pub ceiling_height: f32,
    pub floor_texture: Handle<Image>,
    pub ceiling_texture: Handle<Image>,
    pub id: usize,
}
```

A sector is one room:

- `walls` — the lines that outline it.
- `obstacles` — boxes inside it (the crates/platforms you place).
- `floor_height` / `ceiling_height` — the vertical extent of the room.
- `floor_texture` / `ceiling_texture` — which images cover floor and ceiling.
- `id` — a stable number. **This is not the sector's index in the `sectors`
  vec**; it's a separate identity. Portals refer to each other by `id`, and ids
  can outlive positions in the vec (sectors get deleted and re-added).

### 1.4 `LineDef` — a wall, and when it's a portal

```rust
pub struct LineDef {
    pub start_idx: usize,
    pub end_idx: usize,
    pub front_side_def: SideDef,
    pub back_side_def: Option<SideDef>,
    pub id: WallId,
}
```

- `start_idx` / `end_idx` — indices into `Map.vertices`. The actual positions.
- `front_side_def` — how the wall's front face looks and which sector it
  belongs to.
- `back_side_def` — `None` for a normal solid wall. **`Some(...)` is what makes
  a line a portal** — an opening between two sectors. The back side tells you
  which *other* sector is on the other side.

And `SideDef`:

```rust
pub struct SideDef {
    pub textures: SideDefTextures,
    pub facing: usize,          // which sector id this side belongs to
}

pub struct SideDefTextures {
    pub upper: Option<Handle<Image>>,
    pub middle: Option<Handle<Image>>,
    pub lower: Option<Handle<Image>>,
}
```

A side can have an upper/middle/lower texture (classic Doom-style wall slots).
In this editor, solid walls use `middle`, portals use `upper` + `lower`.

### 1.5 `Obstacle`

```rust
pub struct Obstacle {
    pub id: usize,
    pub edges: Vec<LineDef>,   // a mini wall-list forming the box outline
    pub bottom: f32,
    pub top: f32,
    pub side_texture: Handle<Image>,
    pub top_texture: Handle<Image>,
    pub bottom_texture: Handle<Image>,
}
```

An obstacle is basically a small closed shape: its `edges` are `LineDef`s (so
they too reference pooled vertices), plus three texture slots and a
bottom/top height.

### 1.6 `WallId`

```rust
pub struct WallId {
    pub sector: usize,   // the sector id the wall lives in
    pub index: usize,    // its position in that sector's walls vec
}
```

A stable way to point at one wall. **We won't save this** — it's fully
reconstructible from "which sector" and "which position in the vec", so the
save format can drop it and rebuild it on load.

### 1.7 The one hard part: `Handle<Image>`

Every texture in the model is a `Handle<Image>`. A handle is bevy's way of
referring to a loaded image. The important detail for us:

> A `Handle` is just a process-local **id** (a number that counts up as assets
> are loaded) plus some metadata. It only means something *inside one running
> program*.

Save `Handle(3)` tonight, run a different program tomorrow, and `Handle(3)`
points at nothing — the id number is meaningless across runs. So we cannot put
handles in a file. We must put the **path the image was loaded from**
(`"texture.png"`), and rebuild the handles by loading those paths again.

**This single fact is why the whole design in Part 4 exists.** Everything else
about the map saves cleanly.

---

## Part 2 — The problem, in plain terms

Let's be concrete about why "just serialize the map" fails, and why the answer
is JSON.

### 2.1 Two reasons the naive approach breaks

1. **`Handle<Image>` isn't serializable.** Rust's serialization requires each
   type to say how it converts to/from a data format. `Handle` doesn't — it
   can't meaningfully, because its meaning dies with the process. If you tried
   `serde_json::to_string(&map)` the compiler would tell you: "the trait
   `Serialize` is not implemented for `Handle<Image>`."
2. **jade-ed and jade are separate crates.** Their `Map` types are *different
   Rust types* even though they look identical. There is no single "the Map
   type" that both can serialize. What they *can* share is a **file format**:
   a blob of bytes that both agree on.

### 2.2 Why JSON?

There are three broad options for a file format:

| Format | Human-readable? | Easy to debug? | Notes |
|---|---|---|---|
| JSON (`serde_json`) | Yes | Yes | Simple, widely understood, tiny maps. Best to learn on. |
| TOML (`toml`) | Yes | Yes | Verbose for deeply nested lists (walls/obstacles). |
| Binary (`bincode`/`postcard`) | No | No | Fastest and smallest, but you can't open the file and see what's wrong. |

For map data — which is small — readability and debuggability beat speed. We
choose **JSON**.

### 2.3 What is serde, exactly?

`serde` is a framework for *serialization*. It has two parts:

1. **Traits** — `Serialize` (I can turn myself into a serialized form) and
   `Deserialize` (I can rebuild myself from a serialized form).
2. **The derive macro** — `#[derive(Serialize, Deserialize)]`, which writes
   those implementations for your structs automatically.

`serde` itself doesn't know any format. It's format-agnostic. The *format* is a
separate crate. For JSON, that crate is **`serde_json`**.

> **Analogy:** `serde` is the driver's license — it says "this type can be
> driven through any format". `serde_json` is the car — it knows the roads of
> JSON specifically. serde hands a type to serde_json, and JSON comes out.

You need both: `serde` for the traits + derive, `serde_json` for the JSON
mechanics.

---

## Part 3 — Adding and using the crates

This is the "how do I interact with the newly added crates" part. We'll do it
three times: add the crates, look at what changed, and then **use** them in a
tiny exercise before touching the map.

### 3.1 Adding the crates

`cargo` has a built-in command that adds a dependency to `Cargo.toml` for you.
Run it inside **both** project folders:

```text
cargo add serde --features derive
cargo add serde_json
```

(`--features derive` tells cargo to enable the `derive` feature of `serde`,
which is what provides the `#[derive(...)]` macro. Without it you'd have to
write serialization code by hand.)

Look at what it did to `Cargo.toml`:

```toml
[dependencies]
bevy = { version = "0.19", features = ["dynamic_linking", "debug", "bevy_dev_tools"] }
bevy_egui = "0.41.1"
bevy-inspector-egui = "0.37"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

Two lines appeared. If you prefer editing the file by hand, you can write
exactly that instead of using `cargo add` — they're equivalent. `cargo add`
just also updates `Cargo.lock` for you.

You can also add one and it might already resolve: the lock file already lists
`serde 1.0.229` and `serde_json 1.0.151` because bevy uses them internally. The
important thing is that now *your* code is allowed to `use` them too.

### 3.2 The roles of the two crates, and their imports

| Crate | What it gives you | Import |
|---|---|---|
| `serde` | the `Serialize` / `Deserialize` traits and the `#[derive(...)]` macro | `use serde::{Serialize, Deserialize};` |
| `serde_json` | functions that actually read/write JSON using those traits | `use serde_json;` (often just used via its full path) |

The `serde_json` functions you'll use:

```rust
serde_json::to_string(&value)          // value -> String (compact JSON)
serde_json::to_string_pretty(&value)   // value -> String (indented JSON)
serde_json::from_str::<T>(&text)       // &str -> Result<T, _>
```

Both `to_string` functions return a `Result<String, _>` because serialization
can fail (for example: JSON has no way to represent `NaN`, as we'll see in
Part 11). `from_str` returns a `Result<T, _>` because the text might not match
your type.

### 3.3 Exercise: use the crates for real (before touching the map)

Let's prove we understand the crates by serializing a tiny struct. This also
acts as a smoke test that your setup compiles.

**Step 1.** Create `jade-ed/src/scratch.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
struct SaveRoom {
    name: String,
    floor_height: f32,
    doors: Vec<usize>,          // ids of neighboring sectors
    portal: Option<usize>,      // Some(id) = this side connects to that sector
}

pub fn demo() {
    let room = SaveRoom {
        name: "Main Hall".to_string(),
        floor_height: 0.0,
        doors: vec![0, 1],
        portal: Some(1),
    };

    // SaveRoom -> JSON text
    let json = serde_json::to_string_pretty(&room).expect("serialization failed");
    println!("serialized:\n{json}");

    // JSON text -> SaveRoom
    let back: SaveRoom = serde_json::from_str(&json).expect("deserialization failed");
    println!("round-tripped:\n{back:?}");
}
```

**Step 2.** Declare the module in `src/main.rs` and call the demo once:

```rust
mod scratch;
// ...
fn main() {
    // ... existing app setup ...
    scratch::demo();      // temporary
    App::new() /* ... */.run();
}
```

**Step 3.** Run it:

```text
cargo run
```

You'll see something like:

```text
serialized:
{
  "name": "Main Hall",
  "floor_height": 0.0,
  "doors": [0, 1],
  "portal": 1
}
round-tripped:
SaveRoom { name: "Main Hall", floor_height: 0.0, doors: [0, 1], portal: Some(1) }
```

Look at what just happened — this is the whole trick, in miniature:

- The **derive** generated the serialization code for `SaveRoom`. You wrote no
  serializer.
- `serde_json::to_string_pretty` used that code to produce text.
- `serde_json::from_str` used the mirrored `Deserialize` code to rebuild the
  struct, and it came back *equal* to what we started with.

Notice the field names in the JSON match the struct field names, `Option<usize>`
became `1` when `Some` and would have been `null` when `None`, and `f32` came
out as `0.0`. Everything you need for the real map is here.

**Step 4.** Delete `scratch.rs` and the `mod scratch;` line when you're
satisfied. (Or keep it — but remove the `scratch::demo()` call and the module
before finishing, so the app stays clean.)

> **One thing to know about `Vec2`:** our real `SaveMap` will contain `Vec2`
> values. Is `Vec2` serializable? Yes — `Vec2` is `glam::Vec2`, and bevy
> enables `glam`'s `serde` feature. You can confirm in `Cargo.lock`: the `glam`
> entry lists `serde_core` among its dependencies, which means it was compiled
> with serde support. So we can put `Vec2` straight into our save structs
> without extra conversion. (If you ever want the format to be independent of
> bevy/glam, you could use `[f32; 2]` instead — a plain pair of floats — but
> for two bevy projects there's no need.)

---

## Part 4 — The design: a "disk model" and two conversions

Now we have the tools. The design that solves both problems (handles + two
crates) is:

```
              jade-ed                                jade
   ┌─────────────────────────┐            ┌─────────────────────────┐
   │  Map (runtime)          │            │  Map (runtime)          │
   │  vertices / sectors     │            │  vertices / sectors     │
   │  Handle<Image> textures │            │  Handle<Image> textures │
   └───────────┬─────────────┘            └───────────┬─────────────┘
               │ to_save()                            │ from_save()
               ▼                                      ▼
   ┌─────────────────────────┐            ┌─────────────────────────┐
   │  SaveMap (plain data)   │            │  SaveMap (plain data)   │
   │  vertices / sectors     │            │  vertices / sectors     │
   │  String texture paths   │            │  String texture paths   │
   └───────────┬─────────────┘            └───────────┬─────────────┘
               │ serde_json                            │ serde_json
               ▼                                      ▼
              map.json ─────────────────────────────► map.json
```

The idea is deliberately simple:

- **`SaveMap` is a second, smaller set of types** that mirror the runtime
  model but store **`String` texture paths** instead of `Handle<Image>`. It's
  the *only* thing that touches `serde`, so all our serialization lives in one
  place.
- **`to_save()`** walks a runtime `Map` and produces a `SaveMap` — this is
  where handles become paths.
- **`from_save()`** walks a `SaveMap` and produces a runtime `Map` — this is
  where paths become handles.
- Because `SaveMap` is plain data, it serializes to JSON and back perfectly.

The two projects each keep their own copy of `SaveMap` (about 40 lines). They
don't share code; they share the **format** — both must agree on the field
names and types so a file written by one is read by the other.

### What we keep, what we drop

**Keep** the indexed vertex pool (`start_idx` / `end_idx`). It's the shape the
model already uses, it preserves exact vertex sharing across portals, and
`rebuild_vertices` (`map.rs:388`) would reproduce the same pool if you ever
re-ran it after loading.

**Drop** `WallId { sector, index }` — it's derivable from `(sector id,
position in walls vec)` and can be rebuilt on load. Dropping it keeps the
format small and removes a source of disagreement between the two projects.

### 4.1 The save structs, full definitions

Here is the whole disk model. Put it in a new file `jade-ed/src/save.rs` (and
the identical copy in `jade/src/map/save.rs` — see Parts 7 and 9 for module
wiring):

```rust
use bevy::math::Vec2;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct SaveMap {
    pub vertices: Vec<Vec2>,
    pub sectors: Vec<SaveSector>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SaveSector {
    pub walls: Vec<SaveLine>,
    pub obstacles: Vec<SaveObstacle>,
    pub floor_height: f32,
    pub ceiling_height: f32,
    pub floor_texture: String,   // e.g. "floor_texture.png"
    pub ceiling_texture: String,
    pub id: usize,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SaveLine {
    pub start_idx: usize,
    pub end_idx: usize,
    pub front: SaveSide,
    pub back: Option<SaveSide>, // Some => this line is a portal
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SaveSide {
    pub upper: Option<String>,
    pub middle: Option<String>,
    pub lower: Option<String>,
    pub facing: usize,          // the sector id this side belongs to
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SaveObstacle {
    pub id: usize,
    pub edges: Vec<SaveLine>,
    pub bottom: f32,
    pub top: f32,
    pub side_texture: String,
    pub top_texture: String,
    pub bottom_texture: String,
}
```

Line-by-line:

- `SaveMap` — the top level: the vertex pool and the sectors. Mirrors `Map`.
- `SaveSector` — everything a sector has, but `floor_texture` / `ceiling_texture`
  are **`String`** (the path) instead of `Handle<Image>`.
- `SaveLine` — a wall. `front` is required; `back` is `Option`. **A portal is
  exactly "a line whose `back` is `Some`"** — the same rule as `back_side_def`.
- `SaveSide` — one side's three texture slots (again `String` paths) and the
  sector id it faces.
- `SaveObstacle` — obstacle id, its edges (each a `SaveLine`), heights, and
  three texture paths.

### 4.2 Why this is the "contract"

When jade-ed writes a file and jade reads it, the only thing they agree on is
this JSON shape. If you rename a field in one project but not the other, files
silently read back wrong (or fail to deserialize). So think of `SaveMap` as a
spoken **agreement between two programs** — a contract. In Part 10 we'll add a
`version` field to protect that contract as it evolves.

---

## Part 5 — Converting `Map → SaveMap` (handle → path)

### 5.1 Resolving a handle to its path

Every texture handle in the editor started life as `asset_server.load("path")`.
Bevy's `AssetServer` remembers the reverse mapping: give it a handle id and it
tells you the path it loaded. The method is
[`AssetServer::get_path`](https://docs.rs/bevy/latest/bevy/asset/struct.AssetServer.html#method.get_path):

```rust
use bevy::asset::AssetServer;
use bevy::prelude::*;

fn texture_path(server: &AssetServer, handle: &Handle<Image>) -> String {
    server
        .get_path(handle.id())
        .map(|p| p.to_string())         // "texture.png" (no label)
        .unwrap_or_else(|| "texture.png".to_string())
}
```

What's happening:

- `handle.id()` turns the handle into its raw asset id.
- `server.get_path(id)` returns `Option<AssetPath>`. It's `Option` because a
  handle could in theory belong to something bevy didn't load from a file.
- `.map(|p| p.to_string())` — if we got a path, turn it into a `String`.
- `.unwrap_or_else(|| "texture.png".to_string())` — if we didn't, fall back to
  the known default. (In practice every editor texture resolves fine.)

### 5.2 Writing `to_save`

The rest is mechanical: copy every field, translating the texture slots. The
editor's sector has 2 texture slots, each obstacle has 3, and each side def has
up to 3:

```rust
fn to_save(map: &Map, server: &AssetServer) -> SaveMap {
    SaveMap {
        vertices: map.vertices.clone(),
        sectors: map.sectors.iter().map(|s| SaveSector {
            walls: s.walls.iter().map(|w| line_to_save(w, server)).collect(),
            obstacles: s.obstacles.iter().map(|o| obstacle_to_save(o, server)).collect(),
            floor_height: s.floor_height,
            ceiling_height: s.ceiling_height,
            floor_texture: texture_path(server, &s.floor_texture),
            ceiling_texture: texture_path(server, &s.ceiling_texture),
            id: s.id,
        }).collect(),
    }
}

fn line_to_save(w: &LineDef, server: &AssetServer) -> SaveLine {
    SaveLine {
        start_idx: w.start_idx,
        end_idx: w.end_idx,
        front: side_to_save(&w.front_side_def, server),
        back: w.back_side_def.as_ref().map(|s| side_to_save(s, server)),
    }
}

fn side_to_save(s: &SideDef, server: &AssetServer) -> SaveSide {
    SaveSide {
        upper: s.textures.upper.as_ref().map(|h| texture_path(server, h)),
        middle: s.textures.middle.as_ref().map(|h| texture_path(server, h)),
        lower: s.textures.lower.as_ref().map(|h| texture_path(server, h)),
        facing: s.facing,
    }
}

fn obstacle_to_save(o: &Obstacle, server: &AssetServer) -> SaveObstacle {
    SaveObstacle {
        id: o.id,
        edges: o.edges.iter().map(|w| line_to_save(w, server)).collect(),
        bottom: o.bottom,
        top: o.top,
        side_texture: texture_path(server, &o.side_texture),
        top_texture: texture_path(server, &o.top_texture),
        bottom_texture: texture_path(server, &o.bottom_texture),
    }
}
```

Note the two small decisions here:

- `line_to_save` doesn't need the `AssetServer` because `LineDef` holds no
  handles — only indices and side defs.
- `side_to_save` needs the server but the snippet above abbreviated the call;
  pass `server` down through it so every `upper`/`middle`/`lower` goes through
  `texture_path`. In real code, thread `&AssetServer` through all three helpers.

This is the "boring" part of the whole task, and that's a good sign — when a
conversion is this mechanical, it's usually correct.

---

## Part 6 — Converting `SaveMap → Map` (path → handle)

The reverse direction needs an `AssetServer` so it can load textures:

```rust
fn from_save(save: SaveMap, server: &AssetServer) -> Map {
    let vertices = save.vertices;
    let sectors = save.sectors.into_iter().map(|s| Sector {
        walls: s.walls
            .into_iter()
            .enumerate()
            .map(|(i, w)| line_from_save(w, WallId::new(s.id, i), server))
            .collect(),
        obstacles: s.obstacles
            .into_iter()
            .map(|o| obstacle_from_save(o, s.id, server))
            .collect(),
        floor_height: s.floor_height,
        ceiling_height: s.ceiling_height,
        floor_texture: server.load(&s.floor_texture),
        ceiling_texture: server.load(&s.ceiling_texture),
        id: s.id,
    }).collect();

    Map { vertices, sectors }
}
```

Two things deserve explanation:

**1. Reconstructing `WallId` with `enumerate()`.** We dropped `WallId` from the
file in Part 4. On load we rebuild it: `.enumerate()` gives each wall its
position `i` within the sector, and we make `WallId::new(s.id, i)` — exactly
the "sector id + position in vec" rule we promised.

**2. `server.load`.** Loading a path returns a `Handle`. Crucially, **bevy
deduplicates by path**: calling `server.load("texture.png")` a hundred times
returns the *same* handle every time. That means after loading, all walls share
one wall-texture handle, exactly as they did before saving — and jade's
`material_cache` (which keys materials by handle) works just like it did with
`test_map`.

Where each line becomes:

```rust
fn line_from_save(w: SaveLine, id: WallId, server: &AssetServer) -> LineDef {
    let side = |s: SaveSide| SideDef::new(SideDefTextures {
        upper: s.upper.map(|t| server.load(&t)),
        middle: s.middle.map(|t| server.load(&t)),
        lower: s.lower.map(|t| server.load(&t)),
    }, s.facing);

    LineDef {
        start_idx: w.start_idx,
        end_idx: w.end_idx,
        front_side_def: side(w.front),
        back_side_def: w.back.map(side),
        id,
    }
}
```

Here `side` is a closure (a small inline function) that turns a `SaveSide` into
a `SideDef`. `.map(|t| server.load(&t))` on each `Option<String>` means: "if
there's a path, load it and put the handle here; if there's no path (`None`),
keep it `None`." A portal line comes back as a line with `back_side_def: Some`,
because `w.back.map(side)` returns `Some(SideDef)` when the file had a back
side. The portal-ness survives the trip automatically.

The obstacle conversion is the same pattern: rebuild each edge with
`line_from_save` (give the edges `WallId::new(sector_id, 1000 + ...)` or simply
their position — it only matters to the editor, not the renderer), and load the
three textures.

### Why the other project can now "read it properly"

Put together, the round trip preserves:

- The **vertex pool** exactly (indices unchanged).
- **Portal pairs**: both sectors' portal walls still point at the same pooled
  vertices, because indices are untouched. `rebuild_vertices()` would produce
  the identical pool if you ran it.
- **Sector ids and facing** (`facing` references sector ids, which we saved).
- **Heights and textures** (paths loaded back into deduped handles).

That last line is the answer to "how do I get it to read properly from my other
project": the *format* is the shared thing, and both sides convert through it.

---

## Part 7 — Saving in jade-ed

### 7.1 The save function

```rust
use crate::map::Map;
use serde_json;

pub fn save_map_to_file(map: &Map, server: &AssetServer, path: &str) -> Result<(), String> {
    let save = to_save(map, server);
    let json = serde_json::to_string_pretty(&save).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}
```

- `to_save` builds the disk model (Part 5).
- `serde_json::to_string_pretty` makes indented JSON — much nicer to read in an
  editor when debugging.
- `.map_err(|e| e.to_string())?` converts either error into a `String` and
  returns it early. `save_map_to_file` returns `Result<(), String>` so the
  caller can decide what to do on failure (e.g. show a message).

Where should the file go? A path relative to the crate root lands inside the
`assets/` folder when you run via `cargo run` (CWD is the crate root). So:

```rust
save_map_to_file(&map, &asset_server, "assets/map.json")
```

Putting it in `assets/` matters: jade reads assets from *its* `assets/` folder,
so you can copy `jade-ed/assets/map.json` straight into `jade/assets/map.json`.

### 7.2 Where this code lives

Create `jade-ed/src/save.rs` containing the `SaveMap` types (Part 4), the
`to_save`/`from_save` conversions (Parts 5–6), and these save/load helper
functions. Then declare the module in `src/main.rs`:

```rust
mod save;
```

(There's no `mod` needed in `map.rs`; `save.rs` is a sibling file, attached at
the crate root via `main.rs`, and it imports the map types with
`use crate::map::Map;`.)

### 7.3 Hooking it up: a keyboard shortcut

The simplest hook is a small bevy system that listens for a key:

```rust
fn save_on_key(
    map: Res<Map>,
    asset_server: Res<AssetServer>,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    if keyboard.just_pressed(KeyCode::KeyS) {
        let _ = crate::save::save_map_to_file(&map, &asset_server, "assets/map.json");
    }
}
```

Register it in the editor plugin (`editor.rs`), e.g. alongside the other update
systems: `app.add_systems(Update, save_on_key);`.

### 7.4 Hooking it up: an egui button

If you prefer a button, the `Editor` window in `jade-ed/src/ui.rs:21` is the
place. The function currently reads `mut map: ResMut<Map>`. Add an
`asset_server: Res<AssetServer>` parameter, then inside the window add:

```rust
ui.horizontal(|ui| {
    if ui.button("Save").clicked() {
        let _ = crate::save::save_map_to_file(&map, &asset_server, "assets/map.json");
    }
    if ui.button("Load").clicked() {
        // see Part 8 — a system is better here than inline loading
    }
});
```

For *saving*, calling it inline from the UI works fine because `to_save` only
reads. For *loading* we prefer a system (Part 8) because it needs `Commands` to
swap the resource cleanly. A common pattern is: the button sets a flag in a
small resource, and a system reacts to the flag.

A full file dialog (`rfd` crate) is a nice upgrade later, but a fixed path is
all you need to start.

---

## Part 8 — Loading in jade-ed

### 8.1 The load function and system

One constraint shapes this code: bevy **systems return `()`**, so they can't
use the `?` operator (which needs a function that returns a `Result`). We handle
the "maybe a file, maybe garbage" cases with `let ... else`, which exits the
system early unless a pattern matches:

```rust
fn load_on_key(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    if !keyboard.just_pressed(KeyCode::KeyL) {
        return;
    }
    let Ok(json) = std::fs::read_to_string("assets/map.json") else {
        return; // file missing — do nothing (keep current map)
    };
    let Ok(save) = serde_json::from_str::<SaveMap>(&json) else {
        return; // file unreadable — do nothing
    };
    commands.insert_resource(from_save(save, &asset_server));
}
```

`let Ok(x) = ... else { ... };` is Rust's "if it's not `Ok`, exit early" — when
the expression is `Err`, we return without changing anything; when it's `Ok`,
`x` is bound and we continue. On missing or corrupt files we silently keep the
current map — acceptable for a load key; jade's startup fallback (Part 9) gives
you a better signal there.

The key line is the last one: **`commands.insert_resource(...)`**.

### 8.2 Why `insert_resource` is the right move

The `Map` is a bevy resource. Every system that draws things reads it via
`Res<Map>`. There are two ways to replace it, and they are *not* equivalent:

- `*ResMut<Map> = new_map` — overwrites immediately, in the middle of a frame.
  Dangerous: other systems may hold a borrow of `Map` at that moment.
- `commands.insert_resource(new_map)` — queues the replacement. Bevy applies
  the queued commands at a safe point in the frame (between systems), so there
  are no borrow conflicts.

Always use the second form for loading.

### 8.3 Why the 3D preview rebuilds automatically

This is a subtle but important piece. The 3D preview system
(`map_preview.rs:21`) regenerates all meshes when the map changes:

```rust
if !map.is_changed() && !mode.is_changed() {
    return;  // nothing changed — skip
}
```

`is_changed()` is bevy's change detection: it returns true when the resource
was replaced or mutated since the last time this system ran. When we
`insert_resource` a brand-new `Map`, bevy marks it changed, so the preview
system regenerates everything. **No extra work needed** — the load "just works".

There's one more lucky property: the preview's `material_cache` is a
`Local<HashMap<Handle<Image>, Handle<StandardMaterial>>>` — it caches materials
keyed by image handle. Because `server.load("texture.png")` returns the *same*
handle each run (dedup by path), the new sectors' textures match the already
cached material handles, so the cache stays valid and materials aren't leaked.

### 8.4 Editor polish (optional)

After loading you might want to clear the current `Selection` and let the
vertex-handle gizmos (`map_handles.rs`) re-sync. That's editor UX, not format
work — the data itself is complete after `from_save`.

---

## Part 9 — Loading in jade (the renderer)

jade is simpler than the editor because it builds its meshes **once at
startup** (`spawn_viss_entities` in `render.rs`) and never needs to reload. So
we load the file in `setup_map`, right where `test_map` currently runs.

### 9.1 Where the save code goes

Create `jade/src/map/save.rs` — the *same* `SaveMap` definitions (Part 4) plus
`from_save` (Part 6). It needs the runtime types, which live in
`jade/src/map/mod.rs`, so declare the submodule there:

```rust
// top of jade/src/map/mod.rs
pub mod save;
```

and inside `save.rs` use the parent module's types with:

```rust
use super::{LineDef, Map, Obstacle, Sector, SideDef, SideDefTextures, WallId};
```

(`super::` means "the module above me", i.e. `map`.)

### 9.2 Modify `setup_map`

```rust
fn setup_map(mut commands: Commands, asset_server: Res<AssetServer>) {
    let map = std::fs::read_to_string("assets/map.json")
        .ok()
        .and_then(|json| serde_json::from_str::<SaveMap>(&json).ok())
        .map(|save| from_save(save, &asset_server))
        .unwrap_or_else(|| test_map(&asset_server));

    commands.insert_resource(map);
}
```

Read it as a pipeline:

1. `read_to_string("assets/map.json")` → `Result<String, _>`.
2. `.ok()` → `Option<String>` (missing file = `None`).
3. `.and_then(|json| serde_json::from_str::<SaveMap>(&json).ok())` → parse, and
   swallow parse errors as `None`.
4. `.map(|save| from_save(save, &asset_server))` → convert to a runtime `Map`.
5. `.unwrap_or_else(|| test_map(&asset_server))` → if any step failed, use the
   built-in demo map.

That last fallback is your built-in "did it even load?" indicator: if jade
shows the old test map, the file was missing or didn't parse.

### 9.3 Running it

Both projects read `assets/` relative to their crate root. So:

```text
# from jade-ed: save a map
cargo run            # draw something, press the save key
# copy the result
cp ../jade-ed/assets/map.json assets/map.json
# from jade: run and see your map
cargo run
```

(Or symlink the file: `ln -s ../jade-ed/assets/map.json assets/map.json` so you
never have to copy again.)

That's the whole renderer side: add the dependency, add `save.rs`, wire the
module, change one function. Because jade already loads textures with
`asset_server.load(...)` in `test_map`, the `from_save` texture loading fits
right in with no new machinery.

---

## Part 10 — Keeping the two sides in sync

The JSON format is a **contract between two programs**, so when the map model
changes, both sides must change together. Three rules keep you out of trouble:

1. **Treat the two `save.rs` files as one file.** When you add a field to one
   `Save*` struct, add it to the other, and to `to_save`/`from_save` on both
   sides. Do it in the same edit, so the contract can't drift.

2. **Keep texture paths stable and identical.** The format stores paths, so
   `texture.png` and `floor_texture.png` must exist under the same names in
   both projects' `assets/`. Renaming a texture breaks old save files — exactly
   like renaming any other asset.

3. **Version the format.** The moment you think you might change the schema,
   add a version field:

   ```rust
   pub struct SaveMap {
       pub version: u32,
       pub vertices: Vec<Vec2>,
       pub sectors: Vec<SaveSector>,
   }
   ```

   Set it to `1` when saving. On load, check it and **fail loudly** on an
   unknown version instead of silently misreading a file:

   ```rust
   let map: SaveMap = serde_json::from_str(&json)?;
   if map.version != 1 {
       return Err(format!("unsupported save version {}", map.version));
   }
   ```

A version field is cheap insurance and prevents the worst failure mode in
format evolution: silently wrong maps.

---

## Part 11 — Gotchas

Things that will bite you, and what to do about them:

- **`serde_json` rejects `NaN` / `Infinity`.** JSON has no representation for
  non-finite floats, so serialization errors if any value is non-finite. The
  editor's geometry is finite, but if you ever sanitize input, clamp first:
  `if !x.is_finite() { x = 0.0; }`.

- **Float round-trips are "almost exact".** `serde_json` prints enough digits
  that `f32` values round-trip identically for our purposes, and vertex dedup
  (`add_vertex` compares with `==`) is preserved because both endpoints of a
  wall come from the same value. Don't hand-edit coordinates to more precision
  than serde_json prints — that could make two previously-equal vertices differ.

- **`AssetServer::load` is lazy.** Loading a typo'd path doesn't error; you get
  a broken/empty texture. When a texture "doesn't show up", double-check the
  path string against the actual file in `assets/`.

- **`get_path` only knows loaded handles.** `MapAssets` handles are loaded at
  startup, so they resolve fine. Just don't try to `to_save` before
  `setup_map_assets` has run.

- **Keep the indexed pool.** Never "helpfully" expand walls to raw coordinates
  in the save format. Re-deriving indices is exactly where portals silently
  break.

- **Use deferred insertion for loads.** `commands.insert_resource`, never
  `*ResMut<Map> = ...`, from inside a system that also touches gizmos or other
  systems could borrow the map.

- **Don't put `Handle` in `Save*` structs.** If a struct contains a
  `Handle<Image>` field, it won't compile with `#[derive(Serialize)]`. That
  compile error is the framework telling you you've crossed the boundary.

---

## Part 12 — Verifying it works

### 12.1 A round-trip unit test

The fastest way to catch bugs is a test that doesn't need a window. Add this
under a `#[cfg(test)]` module in `save.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_save_round_trip_preserves_structure() {
        // Build a map (or reuse the test map).
        let map = crate::map::test_map(&crate::map::MapAssets::default());

        // (In a real test you need an AssetServer for to_save/from_save.
        //  If you structure the conversions to take an AssetServer, build a
        //  minimal one or test to_save+from_save over the disk model directly:
        //  to_save -> JSON -> from_str -> from_save.)

        // Assert structurally:
        //  - same vertex count and pool
        //  - same sector ids
        //  - each portal wall's facing pairs still match
        //  - obstacle rects identical
    }
}
```

The spirit: *serialize → deserialize → convert*, then compare the structure.
Even a coarse assertion — "portal facing pairs are the same, vertex pool is the
same" — catches 90% of the bugs in this feature without launching the app.

> Practical note: `to_save`/`from_save` take an `AssetServer`. For a pure unit
> test you may want to split the code so the *shape* is testable without bevy:
> e.g. keep the conversions as pure functions over `SaveMap`, and only resolve
> paths inside the systems that have an `AssetServer`. That's a nice design
> side-effect of the disk-model approach.

### 12.2 Manual checks

1. **Editor round-trip:** run jade-ed, draw a couple of sectors with a portal
   and an obstacle, save, edit the JSON by hand, load — the 2D and 3D views
   should match what you drew.
2. **Renderer:** copy `map.json` into `jade/assets/`, run jade. Walls, floors,
   ceilings, the portal, and the obstacles should render like the old test map
   (or your level). If you instead see the demo map, the file wasn't found.
3. **Round-trip both directions:** save from jade-ed, load in jade, and
   confirm. Then save the *same* file back out of a fresh jade-ed run and diff
   the JSON — it should be essentially identical, proving the format is stable.

---

## Part 13 — Going further: a shared crate

You may have noticed the two projects duplicate not just `SaveMap` but the
entire runtime map model (`Map`, `Sector`, `LineDef`, ...). That duplication
predates saving. Once saving works and you trust the format, the natural
next step is to extract the data model into one crate both projects depend on:

```
jade-map/            # a new library crate
  src/lib.rs         # Map, Sector, LineDef, Obstacle, WallId, SideDef, ...
  src/save.rs        # SaveMap + to_save / from_save + serde
```

Both binaries add it as a **path dependency**:

```toml
[dependencies]
jade-map = { path = "../jade-map" }
```

Then:

- The save format lives in exactly one place; the contract can't drift.
- Both apps keep only their systems and mesh code.
- `to_save`/`from_save` stay with the model, and the renderer side becomes a
  one-line `from_save` call.

This is the payoff of the disk-model design: because the format is already
decoupled from bevy systems, lifting it into a crate is mostly a file move. But
don't do it until the current flow works — a single crate is a refactor, and
it's easier to refactor code you can already run end-to-end.

---

## Glossary

| Term | Meaning |
|---|---|
| crate | a Rust library or program, the unit of compiled code |
| dependency | a crate your project uses, listed in `Cargo.toml` |
| `Cargo.lock` | machine-written record of exact dependency versions |
| package / project | a folder with `Cargo.toml` and `src/` |
| module | a named group of code, usually one file (`mod foo;` attaches `foo.rs`) |
| `use` | brings a name into scope so you can write the short form |
| trait | a set of behaviors a type can implement (`Serialize`, `Deserialize`, `Debug`) |
| derive | a macro that writes a trait implementation for you (`#[derive(...)]`) |
| resource | bevy's shared data (`Res<Map>`, `ResMut<Map>`, `insert_resource`) |
| system | a bevy function that runs each frame and reads/writes resources & entities |
| handle | bevy's reference to a loaded asset; only meaningful within one process |
| `AssetServer` | bevy's asset loader; `load(path) -> Handle`, `get_path(id) -> path` |
| `Vec2` | a 2D vector (a `glam` type); map coordinates `(x, z)` |
| `Option<T>` | `Some(value)` or `None` |
| `Result<T, E>` | `Ok(value)` or `Err(error)` |
| `?` | unwrap `Ok`, return `Err` early from the function |
| indexed vertex pool | walls store indices into one shared vertex list; dedup = shared corners |
| portal | a line whose `back_side_def` / `back` is `Some` — an opening between sectors |
| serialization | converting data to a storable format (here: JSON text) |
| deserialization | converting a stored format back into data |

---

## TL;DR

- `Handle<Image>` is a process-local id and can't be saved. Store **texture
  path strings** instead, and rebuild handles with `AssetServer::load`.
- Define a small **`SaveMap`** disk model (both projects keep a copy — it's the
  format contract) and convert to/from the runtime `Map` with
  `AssetServer::get_path` (handle → path) and `AssetServer::load`
  (path → handle, deduped by path).
- Add the crates with `cargo add serde --features derive` and
  `cargo add serde_json`, then use `#[derive(Serialize, Deserialize)]` and
  `serde_json::to_string_pretty` / `from_str`.
- Save: `to_save` → `serde_json` → `fs::write("assets/map.json")`.
- Load in the editor: `fs::read` → `from_str` → `from_save` →
  `commands.insert_resource(map)` — change detection makes the preview rebuild.
- Load in jade: do it in `setup_map` with `test_map` as fallback.
- Keep the indexed vertex pool; drop `WallId` on disk; add a `version` field
  early; and consider extracting a shared `jade-map` crate once you trust the
  format.
