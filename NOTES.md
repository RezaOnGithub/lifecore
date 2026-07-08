# Info
- Use `cargo +nightly fmt` because we are using nightly `rustfmt` features (but everything else is on stable)
- Warning vs Suggestion
- Edge head == beginning == from
  + Similarly Edge tail == end == to
  + tail means the sharp end of an arrow!

# Pitfalls

- Off-by-one errors, usually confusion regarding the last event in a window.
- We tried to be helpful, and return NodeType on a node, but we return the wrong type because it was overriden
  + That is, the correct type depends on the inspection window AND the node event itself
  + Types can "shift" over time, and Types need to be inspected in context.

# Learning

- Making PayloadExt `#[serde(transparent)]` -- _terrible_ mistake. Wasted hours trying to reconcile data that gets deserialized to Text with Serialized. Just accept that the JSON output is going to be a bit noisy.

# TODO

- Refactor code to use NodeExt and EdgeExt variants of accessors.

- Most `Range<usize>` exists to avoid confusion regarding the final event in a window. The problem is, all
of the Range-based windows are used assuming `window.start == 0`. It wouldn't hurt to assert on that for now.

- Change basic_validation so that MOCK events can reside at the end, and can be arbitrarily many (though emit a Warning that they are present.)

- Some kind of deprecation system for edges? What even is the usecase here?

- Some kind of explicit deprecation system for nodes? Should this even be implemented at type level?


# Backlog

- Gradually phase out `anyhow` in favor of `thiserror`

- Clear choice of words regarding special features: Survivor is a node that is not Stale, Leaf is a node that has only exactly one outgoing edge

- Adding or subtracting from a Range's start and end is not really safe, but who cares?

# WishList

- `EnumAs` trait that panics, and only derivable for `Copy` enums
  - Usecase:
  ```rust
  vec_of_enum.iter().filter(|x| x.is_A()).map(|x| x.as_A())
  ```
- ... `strum` crate seems quite low-maintenance; I should fork it and go crazy
  with additions I want.
  - Better yet, I could use `facet` to implement them.
