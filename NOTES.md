# Conventions

- NEVER derive or implement Debug in production. Prefer
  `#[cfg_attr(debug_assertions, derive(Debug))]` during development if needed.

- Special comment words

| Word       | Meaning                             | Suggested Action                                                                                                                        |
| ---------- | ----------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| SUS        | Suspicious, there may be a bug here | double-check bounds, conditionals, documentation for called fucntions, check for shadowing etc.                                         |
| TODO       | Potential Work To Do                | Indicates future work but is not currently required. Do not engage in the work unless instructed to.                                    |
| NOTDO      | Potential Work You Shouldn't Do     | Indicates work that is deferred indefinitely. Usually sign of a disruptive refactoring we rather not do right now                       |
| COPY_PASTE | Copy-pasted Code                    | Means "code here is copy-pasted and is identical to an excerpt nearby." Typically, change to one instance requires change to the other. |
| NOTE       | Usage Note                          | Something to keep in mind when using or referring to an item                                                                            |
| PERF       | Performance Concern                 | This piece of code may need to be adjusted for better performance. Usually not an immediate concern                                     |

# Info

- Use `cargo +nightly fmt` because we are using nightly `rustfmt` features (but
  everything else is on stable)
- Warning vs Suggestion
- Edge head == beginning == from
  - Similarly Edge tail == end == to
  - tail means the sharp end of an arrow!
- TypeCheck used to be external crate written against Analyzer interfaces; but
  now it is incorporated into the core system

# Pitfalls

- Off-by-one errors, usually confusion regarding the last event in a window.
- We tried to be helpful, and return NodeType on a node, but we return the wrong
  type because it was overriden
  - That is, the correct type depends on the inspection window AND the node
    event itself
  - Types can "shift" over time, and Types need to be inspected in context.

# Learning

- Making PayloadExt `#[serde(transparent)]` -- _terrible_ mistake. Wasted hours
  trying to reconcile data that gets deserialized to Text with Serialized. Just
  accept that the JSON output is going to be a bit noisy.

# Wishlist

- There should be a derive macro that creates a NodeType based on its body
  struct.
  - Basic types (like Timestamp, i64, String) get serialized as their own
  - Option becomes Optional, additional attributes for Encouraged and Deprecated
  - the rest are Serialized if they impl Serialize
  - Otherwise, error.

- Change basic_validation so that MOCK events can reside at the end, and can be
  arbitrarily many (though emit a Warning that they are present.)

- Some kind of deprecation system for edges? What even is the usecase here?

- Some kind of explicit deprecation system for nodes? Should this even be
  implemented at type level?

- Reducing the amount of going over the entire database for validation (much
  much later)

- If only we could encode Analyzer systems in the graph as well, so that they
  have the same lifecycle as the node types they govern. Now, any change to node
  types or the validation logic needs to be made much more carefully.
  - In the future once we know what Analyzers typically look like, we can scrap
    everything and start over I guess...
  - However, it's just less work for now if:
    - We carefully design Analyzer interface.
    - Declare in config strings when is the last event that they apply
    - Fork off onto an entirely new Analyzer

# Backlog

- Gradually phase out `anyhow` in favor of `thiserror`

- Clear choice of words regarding special features: Survivor is a node that is
  not Stale, Leaf is a node that has only exactly one outgoing edge

- Adding or subtracting from a Range's start and end is not really safe, but who
  cares?

# WishList

- `EnumAs` trait that panics, and only derivable for `Copy` enums
  - Usecase:
  ```rust
  vec_of_enum.iter().filter(|x| x.is_A()).map(|x| x.as_A())
  ```
- ... `strum` crate seems quite low-maintenance; I should fork it and go crazy
  with additions I want.
  - Better yet, I could use `facet` to implement them.
